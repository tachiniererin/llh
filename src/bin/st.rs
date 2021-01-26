extern crate reqwest;
extern crate select;
extern crate serde;

use llh as _;

use chrono::Utc;
use select::predicate::{Attr, Class};
use std::time::Instant;
use std::collections::HashMap;

#[tokio::main]
async fn main() -> Result<(), reqwest::Error> {
    println!("Start scraping at {}", Utc::now());
    print!("Main page... ");

    let mut pages: Vec<String> = Vec::new();
    let mut data_pages: HashMap<String, String> = HashMap::new();

    let start = Instant::now();

    llh::get_doc("https://www.st.com")
        .await?
        .find(Class("st-nav__blockmenu-link"))
        .filter_map(|n| n.attr("href"))
        .filter(|a| {
            a.starts_with("/en/") 
            && !a.contains("/applications/")
            && !a.contains("/development-tools/")
            && !a.contains("/embedded-software/")
            && !a.contains("/evaluation-tools/")
        })
        .for_each(|x| pages.push(format!("https://www.st.com{}", x)));

    let duration = start.elapsed();
    println!("took {:?}", duration);

    print!("fetching all sub-categories... ");
    let start = Instant::now();

    for page in pages {
        llh::get_doc(page.as_str())
        .await?
        .find(Attr("name","didyouknow.productId"))
        .filter_map(|n| n.attr("value"))
        .for_each(|x| { 
            let link = String::from(format!("{}.cxst-ps-grid.html/{}.json", page.replace(".html", ""), x));
            data_pages.insert(String::from(x), link);
        });
    }

    let duration = start.elapsed();
    println!("took {:?}", duration);

    print!("fetching all sub-categories... ");
    let start = Instant::now();

    // TODO: save all the JSON data into a database
    for p in data_pages {
        llh::save_json(p.1, format!("json/st/{}.json", p.0)).await?;
    }

    let duration = start.elapsed();
    println!("took {:?}", duration);

    Ok(())
}
