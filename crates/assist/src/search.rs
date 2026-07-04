//! Zero-cost web search for the assist tool loop.
//!
//! Uses DuckDuckGo's HTML endpoint: no API key, no quota, no signup. The
//! page is parsed with plain string scanning so we don't pull in an HTML
//! parser for three CSS classes.

use std::time::Duration;

use anyhow::{Context, Result};

pub struct SearchHit {
    pub title: String,
    pub url: String,
    pub snippet: String,
}

const MAX_HITS: usize = 5;

pub fn web_search(query: &str) -> Result<Vec<SearchHit>> {
    let html = ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(15))
        .build()
        .get("https://html.duckduckgo.com/html/")
        .query("q", query)
        .set(
            "User-Agent",
            "Mozilla/5.0 (X11; Linux x86_64; rv:128.0) Gecko/20100101 Firefox/128.0",
        )
        .call()
        .context("web search unreachable")?
        .into_string()
        .context("web search returned unreadable body")?;
    Ok(parse_results(&html))
}

fn parse_results(html: &str) -> Vec<SearchHit> {
    let mut hits = Vec::new();
    let mut pos = 0;
    while hits.len() < MAX_HITS {
        let Some(anchor) = html[pos..].find("class=\"result__a\"") else {
            break;
        };
        let anchor = pos + anchor;
        let Some(href_off) = html[anchor..].find("href=\"") else {
            break;
        };
        let href_start = anchor + href_off + "href=\"".len();
        let Some(href_len) = html[href_start..].find('"') else {
            break;
        };
        let href = &html[href_start..href_start + href_len];
        let Some(gt) = html[href_start..].find('>') else {
            break;
        };
        let title_start = href_start + gt + 1;
        let Some(title_len) = html[title_start..].find("</a>") else {
            break;
        };
        let title = strip_tags(&html[title_start..title_start + title_len]);
        let after = title_start + title_len;

        let snippet = html[after..]
            .find("result__snippet")
            .and_then(|s| {
                let seg = &html[after + s..];
                let open = seg.find('>')?;
                let close = seg[open..].find("</a>")?;
                Some(strip_tags(&seg[open + 1..open + close]))
            })
            .unwrap_or_default();

        let url = resolve_url(href);
        if !url.is_empty() && !title.is_empty() {
            hits.push(SearchHit {
                title,
                url,
                snippet: truncate(&snippet, 300),
            });
        }
        pos = after;
    }
    hits
}

/// DDG links point at its redirector: `//duckduckgo.com/l/?uddg=<real url>`.
fn resolve_url(href: &str) -> String {
    let href = decode_entities(href);
    if let Some(start) = href.find("uddg=") {
        let raw = &href[start + "uddg=".len()..];
        let raw = raw.split('&').next().unwrap_or(raw);
        return percent_decode(raw);
    }
    if let Some(rest) = href.strip_prefix("//") {
        return format!("https://{rest}");
    }
    href
}

fn strip_tags(html: &str) -> String {
    let mut out = String::with_capacity(html.len());
    let mut in_tag = false;
    for c in html.chars() {
        match c {
            '<' => in_tag = true,
            '>' => in_tag = false,
            c if !in_tag => out.push(c),
            _ => {}
        }
    }
    decode_entities(&out)
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn decode_entities(text: &str) -> String {
    text.replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#x27;", "'")
        .replace("&#39;", "'")
        .replace("&nbsp;", " ")
}

fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'%' if i + 2 < bytes.len() => {
                let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).ok();
                match hex.and_then(|h| u8::from_str_radix(h, 16).ok()) {
                    Some(b) => {
                        out.push(b);
                        i += 3;
                    }
                    None => {
                        out.push(b'%');
                        i += 1;
                    }
                }
            }
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            b => {
                out.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn truncate(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_string();
    }
    let mut out: String = text.chars().take(max).collect();
    out.push_str("...");
    out
}
