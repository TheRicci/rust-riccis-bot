
use reqwest::Error;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use field_accessor::FieldAccessor;

pub struct CoinGeckoClient {
    pub host: &'static str,
}

impl CoinGeckoClient {
    pub fn new(host: &'static str) -> Self {
        CoinGeckoClient { host }
    }

    pub async fn get<R: DeserializeOwned>(&self, endpoint: &str) -> Result<R, Error> {
        reqwest::get(format!("{host}/{ep}", host = self.host, ep = endpoint))
            .await?
            .json()
            .await
    }

    pub async fn coins_list(&self, include_platform: bool) -> Result<Vec<CoinsListItem>, Error> {
        let req = format!("/coins/list?include_platform={}", include_platform);
        self.get(&req).await
    }

    pub async fn coin(
        &self,
        id: &str,
        localization: bool,
        tickers: bool,
        market_data: bool,
        community_data: bool,
        developer_data: bool,
        sparkline: bool,
    ) -> Result<CoinsItem, Error> {
        let req = format!("/coins/{}?localization={}&tickers={}&market_data={}&community_data={}&developer_data={}&sparkline={}", id, localization, tickers, market_data, community_data, developer_data, sparkline);
        self.get(&req).await
    }
  
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct CoinsListItem {
    pub id: String,
    pub symbol: String,
    pub name: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct CoinsItem {
    pub id: String,
    pub symbol: String,
    pub name: String,
    pub image: Image,
    pub market_data: Option<MarketData>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Image {
    pub thumb: Option<String>,
    pub small: Option<String>,
    pub large: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct MarketData {
    pub current_price: CurrencyOption,
    pub ath: CurrencyOption,
    pub atl: CurrencyOption,
    pub market_cap_rank: Value,
    #[serde(rename = "price_change_percentage_1h_in_currency")]
    pub price_change_percentage1_h_in_currency: Option<CurrencyOption>,
    #[serde(rename = "price_change_percentage_24h_in_currency")]
    pub price_change_percentage24_h_in_currency: Option<CurrencyOption>,
}

#[derive(Serialize, Deserialize, Debug, Clone,FieldAccessor)]
pub struct CurrencyOption {
    pub brl: Option<f64>,
    pub btc: Option<f64>,
    pub eth: Option<f64>,
    pub eur: Option<f64>,
    pub usd: Option<f64>,
}

impl CurrencyOption {
    pub fn gets(&self,str:&String)-> Result<&std::option::Option<f64>, std::string::String>{
        self.get(str)
    }
}