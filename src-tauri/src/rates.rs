//! Central Bank of Russia exchange rates for the search bar's converter
//! ("100 usd", "50 € в рублях"): the daily JSON mirror at cbr-xml-daily.ru,
//! no key and reachable from Russia. Kept for an hour, and the last good
//! answer is served when the site can't be reached.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::Serialize;
use serde_json::Value;

use crate::net;

const URL: &str = "https://www.cbr-xml-daily.ru/daily_json.js";
const FRESH: Duration = Duration::from_secs(60 * 60);

#[derive(Serialize, Clone)]
pub struct Rates {
    /// The CBR's date for these rates (ISO).
    date: String,
    /// Roubles per one unit, by ISO code; RUB itself is 1.
    rub: HashMap<String, f64>,
}

static CACHE: Mutex<Option<(Instant, Rates)>> = Mutex::new(None);

fn parse(v: &Value) -> Option<Rates> {
    let mut rub: HashMap<String, f64> = v["Valute"]
        .as_object()?
        .iter()
        .filter_map(|(code, r)| {
            Some((
                code.clone(),
                r["Value"].as_f64()? / r["Nominal"].as_f64().filter(|n| *n > 0.0)?,
            ))
        })
        .collect();
    rub.insert("RUB".into(), 1.0);
    Some(Rates {
        date: v["Date"]
            .as_str()
            .unwrap_or_default()
            .chars()
            .take(10)
            .collect(),
        rub,
    })
}

#[tauri::command]
pub async fn currency_rates() -> Result<Rates, String> {
    if let Some((at, r)) = CACHE.lock().unwrap_or_else(|e| e.into_inner()).clone() {
        if at.elapsed() < FRESH {
            return Ok(r);
        }
    }
    let fetched = async {
        let v: Value = net::client()
            .get(URL)
            .send()
            .await
            .map_err(|e| e.to_string())?
            .json()
            .await
            .map_err(|e| e.to_string())?;
        parse(&v).ok_or_else(|| "Непонятный ответ ЦБ".to_string())
    }
    .await;
    let mut cache = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    match fetched {
        Ok(r) => {
            *cache = Some((Instant::now(), r.clone()));
            Ok(r)
        }
        // Yesterday's rates beat none.
        Err(e) => cache
            .as_ref()
            .map(|(_, r)| r.clone())
            .ok_or(format!("Курсы ЦБ недоступны: {e}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn reads_the_cbr_json() {
        let r = parse(&json!({
            "Date": "2026-09-27T11:30:00+03:00",
            "Valute": {
                "USD": { "Nominal": 1, "Value": 92.5 },
                "JPY": { "Nominal": 100, "Value": 61.0 }
            }
        }))
        .unwrap();
        assert_eq!(r.date, "2026-09-27");
        assert_eq!(r.rub["USD"], 92.5);
        assert!((r.rub["JPY"] - 0.61).abs() < 1e-9);
        assert_eq!(r.rub["RUB"], 1.0);
    }
}
