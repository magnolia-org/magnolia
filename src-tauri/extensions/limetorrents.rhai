fn manifest() {
    #{
        name: "LimeTorrents",
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

        results.push(#{
            title: title,
            page_link: link,
            magnet_link: "",
            size: format_size(parse_int_or(regex_group(`<size>(\d+)</size>`, body), 0)),
            seeds: parse_int_or(regex_group(`Seeds:\s*(\d+)`, desc), 0),
            peers: parse_int_or(regex_group(`Leechers\s*(\d+)`, desc), 0)
        });
    }

    // The RSS feed has no magnet links — fetch them from the detail pages of
    // the 15 best-seeded results.
    results.sort(|a, b| b.seeds - a.seeds);
    results.truncate(15);

    let final_results = [];
    for r in results {
        if !r.page_link.starts_with("http") {
            continue;
        }
        let magnet = "";
        try {
            let page = http_get(r.page_link);
            magnet = regex_group(`href=["'](magnet:\?xt=urn:btih:[^"']+)["']`, page);
        } catch (err) {
            log("LimeTorrents: detail page failed: " + err);
        }
        if magnet == "" {
            continue;
        }
        r.magnet_link = magnet;
        r.remove("page_link");
        final_results.push(r);
    }

    final_results
}
