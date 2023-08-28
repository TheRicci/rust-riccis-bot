use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use reqwest::Error;
use std::env;


pub struct TwelveDataClient {
    pub host: &'static str,
}

impl TwelveDataClient{
    pub fn new(host: &'static str) -> Self {
        TwelveDataClient { host }
    }
    
    pub async fn get<R: DeserializeOwned>(&self, endpoint: &str) -> Result<R, Error> {
        let client = reqwest::Client::new();
        client.get(format!("{}{}", self.host, endpoint))
            .header("X-RapidAPI-Key",env::var("STOCKS_KEY").expect("Expected STOCKS_KEY in the environment"))
            .send()
            .await?
            .json()
            .await
    }

    pub async fn stocks_list(&self) -> Result<DataList, Error> {
        let req = format!("/stocks?format=json");
        self.get(&req).await
    }

    pub async fn stock_quote(&self, symbol: &str, country: &str) -> Result<Stock, Error> {
        let req = format!("/quote?interval=1day&symbol={}&country={}&format=json", symbol,country);
        self.get(&req).await
    }
    
    pub async fn etfs_list(&self) -> Result<DataList, Error> {
        let req = format!("/etf?format=json");
        self.get(&req).await
    }

    pub async fn indices_list(&self) -> Result<DataList, Error> {
        let req = format!("/indices?format=json");
        self.get(&req).await
    }

}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct DataList {
    pub data: Vec<Data>,
    pub status: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Stock {
    pub symbol: String,
    pub name: String,
    pub exchange: String,
    pub open: String,
    pub close: String,
    pub currency: String,
    pub percent_change: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Data{
    pub symbol: String,
    pub name: String,
    pub country: String,
    pub exchange: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ExchangeList {
    pub data: Vec<Exchange>,
    pub status: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Exchange {
    pub code: String,
    pub country: String,
}

