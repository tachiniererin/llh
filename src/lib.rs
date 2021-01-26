extern crate reqwest;
extern crate select;
extern crate serde;

use reqwest::{header::USER_AGENT, Url};
use select::document::Document;
use std::fs::File;
use std::io::prelude::*;
use std::path::Path;

pub async fn get_doc(link: &str) -> Result<Document, reqwest::Error> {
    let client = reqwest::Client::new();
    let res = client
        .get(Url::parse(link).unwrap())
        .header(USER_AGENT, "curl/7.72.0")
        .send()
        .await?;

    let body = res.text().await?;

    Ok(Document::from(body.as_str()))
}

pub async fn save_json(link: String, file_name: String) -> Result<(), reqwest::Error> {
    let path = Path::new(file_name.as_str());
    let display = path.display();

    let mut file = match File::create(&path) {
        Err(why) => panic!("couldn't create {}: {}", display, why),
        Ok(file) => file,
    };

    let client = reqwest::Client::new();
    let res = client
        .get(Url::parse(link.as_str()).unwrap())
        .header(USER_AGENT, "curl/7.72.0")
        .send()
        .await?;

    let body = res.text().await?;

    match file.write_all(body.as_bytes()) {
        Err(why) => panic!("couldn't write to {}: {}", display, why),
        Ok(_) => println!("successfully wrote to {}", display),
    }

    Ok(())
}
