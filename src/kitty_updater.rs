use crate::config_extractor::api_config;
use crate::screen_service::KittyDebt;
use reqwest::Client;
use scraper::{ElementRef, Html, Selector};

#[derive(Debug)]
pub struct KittyUpdater {
    client: Client,
    kitty_url: String,
}

impl KittyUpdater {
    pub fn new(config: &api_config::ApiConfig) -> Result<Self, Box<dyn std::error::Error>> {
        Ok(KittyUpdater {
            client: Client::new(),
            kitty_url: config.kitty.as_ref().ok_or("No Kitty config")?.url.clone(),
        })
    }

    pub async fn get_debts(&self) -> Result<Vec<KittyDebt>, Box<dyn std::error::Error>> {
        let body = self
            .client
            .get(&self.kitty_url)
            .send()
            .await?
            .text()
            .await?;

        extract_debts(&body).map_err(|err| format!("Error parsing Kitty debts: {:?}", err).into())
    }
}

fn extract_debts(body: &String) -> Result<Vec<KittyDebt>, Box<dyn std::error::Error>> {
    // Parse the body into a tree structure, returning on parsing errors
    let parsed_body = Html::parse_document(&body);

    // Select elements like '<div class="transaction-text">'
    let transaction_selector = Selector::parse(r#"div[class="transaction-text"]"#)?;
    // Try to extract a proper debt from each of them, discarding failures (but logging them)
    let debts: Vec<KittyDebt> = parsed_body
        .select(&transaction_selector)
        .filter_map(|t| {
            extract_debt(&t)
                // TODO: log this better than a println
                .inspect_err(|e| println!("Error extracting a debt: {}", e))
                .ok()
        })
        .collect();

    // Error out if we didn't find a single debt (there should really always be two)
    if debts.is_empty() {
        return Err(format!(
            "No debts found on page (body size: {}, parse errors: [{}])",
            body.len(),
            parsed_body.errors.join(", ")
        )
        .into());
    }

    Ok(debts)
}

fn extract_debt(element: &ElementRef) -> Result<KittyDebt, Box<dyn std::error::Error>> {
    let all_texts = element.text().collect::<Vec<_>>();

    let who_text = all_texts
        .iter()
        .find(|t| t.contains(" gives "))
        .ok_or("no text node contained 'gives'")?
        .replace(" gives ", "");
    let who = who_text.trim().to_string();
    let how_much = all_texts
        .iter()
        .find_map(|t| t.trim().parse::<f32>().ok())
        .ok_or("no text field was parseable into a float")?;
    let whom_text = all_texts
        .iter()
        .find(|t| t.contains(" to "))
        .ok_or("no text node contained 'to'")?
        .replace(" to ", "");
    let whom = whom_text.trim().to_string();

    if who.is_empty() || whom.is_empty() {
        return Err(format!("either who ('{}') or whom ('{}') was empty", who, whom).into());
    }
    Ok(KittyDebt {
        who,
        how_much,
        whom,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_one_debt() {
        let body = r#"
<body>
<ul class="transactions horizontal-divider">
    <li class="transaction ks-data-row">
        <div class="transaction-icon kitty-icon-column">
            <i class="fa-icon fas fa-money-bill-alt text-success " aria-hidden="true"></i>
        </div>
        <div class="transaction-text">
            Sid gives <span class="currency"><span class="currency-symbol">CHF</span>72.50</span> to Moses
        </div>
        <div class="transaction-action">
        </div>
    </li>
</ul>
</body>
"#.into();
        let expected = KittyDebt {
            who: "Sid".into(),
            how_much: 72.5,
            whom: "Moses".into(),
        };
        assert_eq!(extract_debts(&body).unwrap(), vec![expected]);
    }

    #[test]
    fn finds_two_debts() {
        let body = r#"
<body>
<ul class="transactions horizontal-divider">
    <li class="transaction ks-data-row">
        <div class="transaction-icon kitty-icon-column">
            <i class="fa-icon fas fa-money-bill-alt text-success " aria-hidden="true"></i>
        </div>
        <div class="transaction-text">
            Sid gives <span class="currency"><span class="currency-symbol">CHF</span>72.50</span> to Moses
        </div>
        <div class="transaction-action">
        </div>
    </li>
    <li class="transaction ks-data-row">
        <div class="transaction-icon kitty-icon-column">
            <i class="fa-icon fas fa-certificate text-muted " aria-hidden="true"></i>
        </div>
        <div class="transaction-text">
            Bini gives <span class="currency"><span class="currency-symbol">CHF</span>137.94</span> to Moses
        </div>
        <div class="transaction-action">
        </div>
    </li>
</ul>
</body>
"#.into();
        let expected_one = KittyDebt {
            who: "Sid".into(),
            how_much: 72.5,
            whom: "Moses".into(),
        };
        let expected_two = KittyDebt {
            who: "Bini".into(),
            how_much: 137.94,
            whom: "Moses".into(),
        };
        assert_eq!(
            extract_debts(&body).unwrap(),
            vec![expected_one, expected_two]
        );
    }

    #[test]
    fn doesnt_panic_on_garbled_input() {
        let body = "\\<".into();
        assert!(extract_debts(&body).is_err());
    }

    #[test]
    fn doesnt_panic_on_empty_page() {
        let body = "".into();
        assert!(extract_debts(&body).is_err());
    }

    #[test]
    fn doesnt_panic_on_no_debts() {
        let body = r#"
<body>
<ul class="transactions horizontal-divider">
    <li class="transaction ks-data-row">
        <div class="transaction-icon kitty-icon-column">
            <i class="fa-icon fas fa-money-bill-alt text-success " aria-hidden="true"></i>
        </div>
        <div class="transaction-action">
        </div>
    </li>
    <li class="transaction ks-data-row">
        <div class="transaction-icon kitty-icon-column">
            <i class="fa-icon fas fa-certificate text-muted " aria-hidden="true"></i>
        </div>
        <div class="transaction-action">
        </div>
    </li>
</ul>
</body>
"#
        .into();
        assert!(extract_debts(&body).is_err());
    }
}
