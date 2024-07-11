use crate::config_extractor::api_config;

use crate::screen_service::KittyDebt;
use reqwest::Client;

pub async fn get_debts(config: &api_config::ApiConfig) -> Result<Vec<KittyDebt>, Box<dyn std::error::Error>> {
    let mut debts = vec![];
    let kitty_url = config.kitty.as_ref().ok_or("no Kitty config")?.url.clone();
    let client = Client::new(); // TODO: move this outside (member var?) to avoid creating a client at each call
    let body = client.get(kitty_url).send().await?.text().await?;
    debts.push(KittyDebt {
        who: "".into(),
        how_much: 0.0,
        whom: "".into(),
    });
    Ok(debts)
}

fn extract_debt(body: &String) -> Option<KittyDebt> {
    let who_start = body.find("<div class=\"transaction-text\">")?;
    println!("who_start = {who_start}");

    let who_end = body[who_start..].find("<span class=\"currency\">")?;
    let how_much_start = who_end + 4;
    let how_much_end = body[how_much_start..].find("</span>")?;
    let whom_start = how_much_end + 6;
    let whom_end = body[who_start..].find("</div>")?;

    let who = &body[who_start..who_end];
    let how_much = &body[how_much_start..how_much_end].parse::<f32>().ok()?;
    let whom = &body[whom_start..whom_end];

    let debt = KittyDebt {
        who: who.into(),
        how_much: *how_much,
        whom: whom.into(),
    };
    Some(debt)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_first_debt() {
        let body = r#"
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
"#.into();
        let expected = KittyDebt {
            who: "Sid".into(),
            how_much: 72.5,
            whom: "Moses".into()
        };
        assert_eq!(extract_debt(&body), Some(expected));
    }
}
