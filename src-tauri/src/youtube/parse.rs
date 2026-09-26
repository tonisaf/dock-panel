//! Pure parsing for the YouTube widget: channel feeds (Atom), channel IDs in
//! pages and links, and Takeout's subscriptions.csv. No I/O, so it is tested
//! on any platform.

use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Video {
    pub id: String,
    pub channel_id: String,
    pub channel_title: String,
    pub title: String,
    /// Unix ms.
    pub published: i64,
    pub thumbnail: Option<String>,
    pub views: Option<u64>,
    pub short: bool,
    pub url: String,
}

/// "2026-09-25T15:00:07+00:00" (RFC 3339, as in the feeds) → Unix ms.
pub fn parse_time(s: &str) -> Option<i64> {
    let b = s.as_bytes();
    let num = |from: usize, len: usize| s.get(from..from + len)?.parse::<i64>().ok();
    let (y, mo, d, h, mi, se) = (num(0, 4)?, num(5, 2)?, num(8, 2)?, num(11, 2)?, num(14, 2)?, num(17, 2)?);
    // Days from civil (Howard Hinnant), valid for the proleptic Gregorian calendar.
    let yy = if mo <= 2 { y - 1 } else { y };
    let era = yy.div_euclid(400);
    let yoe = yy - era * 400;
    let doy = (153 * (if mo > 2 { mo - 3 } else { mo + 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    let mut secs = days * 86_400 + h * 3600 + mi * 60 + se;
    // Skip fractional seconds, then apply the offset.
    let mut i = 19;
    if b.get(i) == Some(&b'.') {
        i += 1;
        while b.get(i).is_some_and(u8::is_ascii_digit) {
            i += 1;
        }
    }
    match b.get(i) {
        Some(b'+') | Some(b'-') => {
            let sign = if b[i] == b'+' { 1 } else { -1 };
            secs -= sign * (num(i + 1, 2)? * 3600 + num(i + 4, 2)? * 60);
        }
        Some(b'Z') | None => {}
        _ => return None,
    }
    Some(secs * 1000)
}

const ATOM: &str = "http://www.w3.org/2005/Atom";
const YT: &str = "http://www.youtube.com/xml/schemas/2015";
const MEDIA: &str = "http://search.yahoo.com/mrss/";

fn child<'a, 'i>(n: roxmltree::Node<'a, 'i>, ns: &str, name: &str) -> Option<roxmltree::Node<'a, 'i>> {
    n.children().find(|c| c.has_tag_name((ns, name)))
}

fn text(n: Option<roxmltree::Node<'_, '_>>) -> String {
    n.and_then(|n| n.text()).unwrap_or_default().trim().to_string()
}

/// A channel's Atom feed → its title and videos, newest first.
pub fn parse_feed(xml: &str) -> Result<(String, Vec<Video>), String> {
    let doc = roxmltree::Document::parse(xml).map_err(|e| format!("непонятный ответ YouTube: {e}"))?;
    let root = doc.root_element();

    let channel_title = text(child(root, ATOM, "title"));
    let channel_id = text(child(root, YT, "channelId"));
    let mut videos: Vec<Video> = root
        .children()
        .filter(|n| n.has_tag_name((ATOM, "entry")))
        .filter_map(|e| {
            let id = text(child(e, YT, "videoId"));
            if id.is_empty() {
                return None;
            }
            let link = child(e, ATOM, "link").and_then(|l| l.attribute("href")).unwrap_or_default().to_string();
            let group = child(e, MEDIA, "group");
            let thumbnail = group.and_then(|g| child(g, MEDIA, "thumbnail")).and_then(|t| t.attribute("url")).map(str::to_string);
            let views = group
                .and_then(|g| child(g, MEDIA, "community"))
                .and_then(|c| child(c, MEDIA, "statistics"))
                .and_then(|s| s.attribute("views"))
                .and_then(|v| v.parse().ok());
            let entry_channel = text(child(e, YT, "channelId"));
            Some(Video {
                short: link.contains("/shorts/"),
                url: if link.is_empty() { format!("https://www.youtube.com/watch?v={id}") } else { link },
                channel_id: if entry_channel.is_empty() { channel_id.clone() } else { entry_channel },
                channel_title: channel_title.clone(),
                title: text(child(e, ATOM, "title")),
                published: parse_time(&text(child(e, ATOM, "published"))).unwrap_or(0),
                thumbnail,
                views,
                id,
            })
        })
        .collect();
    videos.sort_by(|a, b| b.published.cmp(&a.published));
    Ok((channel_title, videos))
}

pub fn is_channel_id(s: &str) -> bool {
    s.len() == 24 && s.starts_with("UC") && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

/// The channel ID a channel, handle or video page declares.
pub fn channel_id_in_page(html: &str) -> Option<String> {
    const MARKERS: [&str; 4] = [
        "<link rel=\"canonical\" href=\"https://www.youtube.com/channel/",
        "\"externalId\":\"",
        "<meta itemprop=\"channelId\" content=\"",
        "\"channelId\":\"",
    ];
    MARKERS.iter().find_map(|m| {
        let at = html.find(m)? + m.len();
        let id = html.get(at..at + 24)?;
        is_channel_id(id).then(|| id.to_string())
    })
}

/// Channel IDs and titles from Takeout's subscriptions.csv
/// ("Channel Id,Channel Url,Channel Title", in any UI language).
pub fn parse_takeout(csv: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for line in csv.lines() {
        let fields = split_csv_line(line);
        let Some(id) = fields.iter().find(|f| is_channel_id(f.trim())) else { continue };
        let title = fields.last().map(|t| t.trim()).filter(|t| !t.is_empty() && !is_channel_id(t) && !t.starts_with("http"));
        out.push((id.trim().to_string(), title.unwrap_or(id).to_string()));
    }
    out
}

fn split_csv_line(line: &str) -> Vec<String> {
    let (mut fields, mut cur, mut quoted) = (Vec::new(), String::new(), false);
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '"' if quoted && chars.peek() == Some(&'"') => {
                cur.push('"');
                chars.next();
            }
            '"' => quoted = !quoted,
            ',' if !quoted => fields.push(std::mem::take(&mut cur)),
            c => cur.push(c),
        }
    }
    fields.push(cur);
    fields
}

#[cfg(test)]
mod tests {
    use super::*;

    const FEED: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<feed xmlns:yt="http://www.youtube.com/xml/schemas/2015" xmlns:media="http://search.yahoo.com/mrss/" xmlns="http://www.w3.org/2005/Atom">
 <link rel="self" href="http://www.youtube.com/feeds/videos.xml?channel_id=UC_x5XG1OV2P6uZZ5FSM9Ttw"/>
 <id>yt:channel:_x5XG1OV2P6uZZ5FSM9Ttw</id>
 <yt:channelId>_x5XG1OV2P6uZZ5FSM9Ttw</yt:channelId>
 <title>Google for Developers</title>
 <link rel="alternate" href="https://www.youtube.com/channel/UC_x5XG1OV2P6uZZ5FSM9Ttw"/>
 <author><name>Google for Developers</name><uri>https://www.youtube.com/channel/UC_x5XG1OV2P6uZZ5FSM9Ttw</uri></author>
 <published>2007-08-23T00:34:43+00:00</published>
 <entry>
  <id>yt:video:aaaaaaaaaaa</id>
  <yt:videoId>aaaaaaaaaaa</yt:videoId>
  <yt:channelId>UC_x5XG1OV2P6uZZ5FSM9Ttw</yt:channelId>
  <title>Older &amp; longer</title>
  <link rel="alternate" href="https://www.youtube.com/watch?v=aaaaaaaaaaa"/>
  <published>2026-09-24T15:00:07+00:00</published>
  <media:group>
   <media:title>Older</media:title>
   <media:thumbnail url="https://i1.ytimg.com/vi/aaaaaaaaaaa/hqdefault.jpg" width="480" height="360"/>
   <media:community><media:starRating count="10" average="5.00" min="1" max="5"/><media:statistics views="12345"/></media:community>
  </media:group>
 </entry>
 <entry>
  <id>yt:video:bbbbbbbbbbb</id>
  <yt:videoId>bbbbbbbbbbb</yt:videoId>
  <yt:channelId>UC_x5XG1OV2P6uZZ5FSM9Ttw</yt:channelId>
  <title>Новый шорт</title>
  <link rel="alternate" href="https://www.youtube.com/shorts/bbbbbbbbbbb"/>
  <published>2026-09-25T18:30:00+03:00</published>
  <media:group><media:thumbnail url="https://i2.ytimg.com/vi/bbbbbbbbbbb/hqdefault.jpg" width="480" height="360"/></media:group>
 </entry>
</feed>"#;

    #[test]
    fn parses_feed() {
        let (title, videos) = parse_feed(FEED).unwrap();
        assert_eq!(title, "Google for Developers");
        assert_eq!(videos.len(), 2);
        assert_eq!(videos[0].id, "bbbbbbbbbbb", "newest first");
        assert!(videos[0].short);
        assert_eq!(videos[0].published, parse_time("2026-09-25T15:30:00Z").unwrap());
        assert_eq!(videos[1].title, "Older & longer");
        assert_eq!(videos[1].views, Some(12345));
        assert_eq!(videos[1].channel_id, "UC_x5XG1OV2P6uZZ5FSM9Ttw");
        assert!(videos[1].thumbnail.as_deref().unwrap().contains("hqdefault"));
    }

    #[test]
    fn parses_times() {
        assert_eq!(parse_time("1970-01-01T00:00:00+00:00"), Some(0));
        assert_eq!(parse_time("2000-03-01T00:00:00Z"), Some(951_868_800_000));
        assert_eq!(parse_time("2026-09-25T18:30:00.123+03:00"), parse_time("2026-09-25T15:30:00Z"));
        assert_eq!(parse_time("garbage"), None);
    }

    #[test]
    fn finds_channel_id_in_pages() {
        let handle = r#"<html><link rel="canonical" href="https://www.youtube.com/channel/UC_x5XG1OV2P6uZZ5FSM9Ttw"></html>"#;
        assert_eq!(channel_id_in_page(handle).as_deref(), Some("UC_x5XG1OV2P6uZZ5FSM9Ttw"));
        let video = r#"<meta itemprop="channelId" content="UCBR8-60-B28hp2BmDPdntcQ">"#;
        assert_eq!(channel_id_in_page(video).as_deref(), Some("UCBR8-60-B28hp2BmDPdntcQ"));
        assert_eq!(channel_id_in_page("<html>nothing</html>"), None);
    }

    #[test]
    fn parses_takeout_csv() {
        let csv = "Идентификатор канала,URL канала,Название канала\n\
            UC_x5XG1OV2P6uZZ5FSM9Ttw,http://www.youtube.com/channel/UC_x5XG1OV2P6uZZ5FSM9Ttw,Google for Developers\n\
            UCBR8-60-B28hp2BmDPdntcQ,http://www.youtube.com/channel/UCBR8-60-B28hp2BmDPdntcQ,\"YouTube, official\"\n\n";
        let channels = parse_takeout(csv);
        assert_eq!(channels.len(), 2);
        assert_eq!(channels[1].0, "UCBR8-60-B28hp2BmDPdntcQ");
        assert_eq!(channels[1].1, "YouTube, official");
    }
}
