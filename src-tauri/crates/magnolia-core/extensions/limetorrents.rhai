fn manifest() {
    #{
        name: "LimeTorrents",
        version: "2.0.0",
        author: "Magnolia",
        type: "tracker",
        tracker_name: "LimeTorrents",
        is_anime: false,
        fields: []
    }
}

fn search(ctx) {
    let url = "https://www.limetorrents.fun/searchrss/" + url_encode(ctx.query) + "/";
    let xml = http_get(url);

    let results = [];
    let missing = [];
    for item in regex_capture_all(`(?s)<item>(.*?)</item>`, xml) {
        let body = item[1];

        let category = regex_group(`<category>(?:<!\[CDATA\[)?(.*?)(?:\]\]>)?</category>`, body);
        category.trim();
        if category != "Movies" && category != "TV shows" && category != "Anime" {
            continue;
        }

        let title = html_unescape(regex_group(`<title>(?:<!\[CDATA\[)?(.*?)(?:\]\]>)?</title>`, body));
        title.trim();
        if title == "" {
            continue;
        }

        let link = regex_group(`<link>(?:<!\[CDATA\[)?(.*?)(?:\]\]>)?</link>`, body);
        link.trim();
        let desc = regex_group(`<description>(?:<!\[CDATA\[)?(.*?)(?:\]\]>)?</description>`, body);

        let result = #{
            title: title,
            info_hash: regex_group(`/torrent/([A-Fa-f0-9]{40})\.torrent`, body),
            size_bytes: parse_int_or(regex_group(`<size>(\d+)</size>`, body), 0),
            seeds: parse_int_or(regex_group(`Seeds:\s*(\d+)`, desc), 0),
            peers: parse_int_or(regex_group(`Leechers\s*(\d+)`, desc), 0),
            uploaded: parse_date(regex_group(`<pubDate>(.*?)</pubDate>`, body))
        };
        if result.info_hash != "" {
            results.push(result);
        } else if link.starts_with("http") {
            result.page_link = link;
            missing.push(result);
        }
    }

    // the feed's enclosure carries the info hash; only items without one need
    // their detail page, fetched all at once for the best seeded few
    missing.sort(|a, b| b.seeds - a.seeds);
    missing.truncate(15);
    let pages = http_get_all(missing.map(|r| r.page_link));
    for i in 0..missing.len() {
        let r = missing[i];
        let magnet = regex_group(`href=["'](magnet:\?xt=urn:btih:[^"']+)["']`, pages[i]);
        if magnet == "" {
            continue;
        }
        r.remove("page_link");
        r.remove("info_hash");
        r.magnet_link = magnet;
        results.push(r);
    }

    results
}
