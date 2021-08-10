extern crate reqwest;
extern crate select;
extern crate serde;

use llh as _;

use chrono::Utc;
use futures::{stream, StreamExt};
use indicatif::{ProgressBar, ProgressStyle};
use select::predicate::{Attr, Class, Name};
use serde::Deserialize;
use std::collections::HashMap;
use std::fs::File;
use std::path::Path;
use std::time::Instant;

#[derive(Deserialize)]
#[allow(dead_code)]
struct Category {
    columns: Vec<HashMap<String, serde_json::Value>>,
    rows: Vec<Product>,
    #[serde(alias = "levelTitle")]
    level_title: String,
    breadcrumb: String,
}

#[derive(Deserialize, Clone)]
struct Product {
    #[serde(alias = "productId")]
    product_id: String,
    path: String,
    cells: Vec<HashMap<String, String>>,
    #[serde(alias = "productFolderUrl")]
    product_folder_url: String,
    #[serde(alias = "availableInDistributorStock")]
    available_in_distributor_stock: bool,
    #[serde(alias = "availableAsFreeSample")]
    available_as_free_sample: bool,
    #[serde(alias = "newProductIntroduction")]
    new_product_introduction: bool,
    #[serde(alias = "isNewProduct")]
    is_new_product: bool,
    #[serde(alias = "isPublic")]
    is_public: bool,
}

#[tokio::main]
async fn main() -> Result<(), reqwest::Error> {
    let pb_style = ProgressStyle::default_bar()
        .template("{msg} {bar:40.magenta/blue} {pos}/{len} ({eta})")
        .progress_chars("##-");

    println!("Start scraping NXP at {}", Utc::now());
    print!("Fetching main page... ");

    let mut pages: Vec<String> = Vec::new();

    llh::get_doc("https://www.nxp.com")
        .await?
        .find(Name("a"))
        .filter_map(|n| n.attr("href"))
        .filter(|a| {
            a.starts_with("/products/")
                && !a.contains("?")
        })
        .for_each(|x| pages.push(String::from(x)));

    for page in pages {
        println!("{}", page);
    }

    Ok(())
}
