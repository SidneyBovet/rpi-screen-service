use crate::screen_service::{KittyDebt, ScreenContentReply};
use crate::{config_extractor::api_config, data_updater::DataUpdater};
use log::{error, info, warn};
use reqwest::Client;
use scraper::{ElementRef, Html, Selector};
use std::sync::{Arc, Mutex};
use tokio::time::{Duration, Instant};

#[derive(Debug)]
// We switch from one to the other for manual testing, but it's actually fine to keep both.
#[allow(dead_code)]
pub enum KittyUpdateMode {
    Dummy,
    Real,
}

#[derive(Debug)]
pub struct KittyUpdater {
    update_mode: KittyUpdateMode,
    client: Client,
    kitty_url: String,
    kitty_period: Duration,
}

#[tonic::async_trait]
impl DataUpdater for KittyUpdater {
    fn get_next_update_time(&self) -> Instant {
        match self.update_mode {
            KittyUpdateMode::Dummy => Instant::now() + Duration::from_secs(19),
            KittyUpdateMode::Real => Instant::now() + self.kitty_period,
        }
    }

    async fn update(&mut self, screen_content: &Arc<Mutex<ScreenContentReply>>) {
        info!("Updating {:?} Kitty", self.update_mode);
        let debts;
        match self.update_mode {
            KittyUpdateMode::Dummy => {
                let now = chrono::offset::Local::now();
                let now_seconds =
                    // These have no rigth to fail, since 'sec' is between 0 and 59
                    f32::try_from(u16::try_from(chrono::Timelike::second(&now)).unwrap()).unwrap();
                debts = vec![KittyDebt {
                    who: "foo".into(),
                    how_much: now_seconds,
                    whom: "bar".into(),
                }]
            }
            KittyUpdateMode::Real => {
                debts = match self.get_debts().await {
                    Ok(d) => d,
                    Err(e) => {
                        error!("Error getting Kitty debts: {}", e);
                        vec![]
                    }
                }
            }
        };
        match screen_content.lock() {
            Ok(mut content) => {
                content.kitty_debts = debts;
            }
            Err(e) => error!("Poisoned lock when writing debts: {}", e),
        };
    }
}

impl KittyUpdater {
    pub fn new(
        update_mode: KittyUpdateMode,
        config: &api_config::ApiConfig,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let kitty_config = config.kitty.as_ref().ok_or("No kitty config")?;
        let kitty_url = kitty_config.url.clone();
        let kitty_period = Duration::from_secs(
            kitty_config
                .update_period
                .as_ref()
                .ok_or("no kitty update period")?
                .seconds
                .try_into()?,
        );
        Ok(KittyUpdater {
            update_mode,
            client: Client::new(),
            kitty_url,
            kitty_period,
        })
    }

    async fn get_debts(&self) -> Result<Vec<KittyDebt>, Box<dyn std::error::Error>> {
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
                .inspect_err(|e| {
                    warn!(
                        "Error extracting a debt from a 'transaction-text' div: {}",
                        e
                    )
                })
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
