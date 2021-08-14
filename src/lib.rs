extern crate lazy_static;
extern crate reqwest;
extern crate select;
extern crate serde;

use indicatif::ProgressStyle;
use reqwest::{header::USER_AGENT, StatusCode, Url};
use select::document::Document;
use serde_json::json;
use std::fs::File;
use std::io::prelude::*;
use std::path::Path;

pub const VERSION: &'static str = env!("CARGO_PKG_VERSION");

lazy_static::lazy_static! {
    pub static ref PB_STYLE: ProgressStyle = ProgressStyle::default_bar()
        .template("{msg} {bar:40.magenta/blue} {pos}/{len} ({eta})")
        .progress_chars("##-");
}

pub async fn get_doc(link: &str) -> Result<Document, reqwest::Error> {
    let client = reqwest::Client::new();
    let res = client
        .get(Url::parse(link).unwrap())
        .header(USER_AGENT, "curl/7.74.0")
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
        .header(USER_AGENT, "curl/7.74.0")
        .send()
        .await?;

    let body = res.text().await?;
    // TODO: make it async and pretty too
    let v: serde_json::Value = match serde_json::from_str(&body) {
        Err(why) => {
            eprintln!("couldn't parse {}: {}", link, why);
            return Ok(());
        }
        Ok(v) => v,
    };

    match file.write_all(serde_json::to_string_pretty(&v).unwrap().as_bytes()) {
        Err(why) => panic!("couldn't write to {}: {}", display, why),
        Ok(_) => return Ok(()),
    }
}

pub async fn save_pdf(link: String, file_name: String) -> Result<(), reqwest::Error> {
    let path = Path::new(file_name.as_str());
    let display = path.display();
    // let path_temp = format!("{}.1", file_name);

    // skip already downloaded PDFs for now
    if Path::new(&path).exists() {
        // path = Path::new(path_temp.as_str());
        return Ok(());
    }

    let mut file = match File::create(&path) {
        Err(why) => panic!("couldn't create {}: {}", display, why),
        Ok(file) => file,
    };

    let client = reqwest::Client::new();
    let res = client
        .get(Url::parse(link.as_str()).unwrap())
        .header(USER_AGENT, "curl/7.74.0")
        .send()
        .await?;

    let res = match res.error_for_status() {
        Ok(res) => res,
        Err(why) => match why.status().unwrap() {
            StatusCode::NOT_FOUND => return Ok(()),
            StatusCode::FORBIDDEN => panic!("url {}: 403", link),
            default => panic!("unhandled request error for {}: {}", link, default),
        },
    };

    let body = res.bytes().await?;

    match file.write_all(&body) {
        Err(why) => panic!("couldn't write to {}: {}", display, why),
        Ok(_) => return Ok(()),
    }
}

pub async fn empty() -> Result<(), reqwest::Error> {
    Ok(())
}

pub fn dump_json<T: serde::Serialize>(f: &str, m: T) {
    let path = Path::new(f);
    let display = path.display();

    let mut file = match File::create(&path) {
        Err(why) => panic!("couldn't create {}: {}", display, why),
        Ok(file) => file,
    };
    match file.write_fmt(format_args!("{}", json!(m))) {
        Err(why) => panic!("couldn't write to {}: {}", display, why),
        Ok(_) => println!("successfully wrote to {}", display),
    }
}
