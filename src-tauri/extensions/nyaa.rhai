fn manifest() {
    #{
        name: "Nyaa",
        author: "Magnolia",
        type: "tracker",
        tracker_name: "Nyaa",
        is_anime: true,
        fields: []
    }
}

fn search(ctx) {
    let trackers = "&tr=" + url_encode("http://nyaa.tracker.wf:7777/announce")
        + "&tr=" + url_encode("udp://tracker.opentrackr.org:1337/announce")
        + "&tr=" + url_encode("udp://open.stealth.si:80/announce")
        + "&tr=" + url_encode("udp://exodus.desync.com:6969/announce")
        + "&tr=" + url_encode("udp://tracker.torrent.eu.org:451/announce");

    let results = [];

    // First 3 pages (75 results per page)
    for page in 1..4 {
        let url = "https://nyaa.si/?page=rss&f=0&c=1_0&q=" + url_encode(ctx.query)
            + "&s=seeders&o=desc&p=" + page;
        let xml = "";
        try {
            xml = http_get(url);
        } catch (err) {
            log("Nyaa: page " + page + " failed: " + err);
            break;
        }

        let items = regex_capture_all(`(?s)<item>(.*?)</item>`, xml);
        if items.len() == 0 {
            break;
        }

        for item in items {
            let body = item[1];
            let title = html_unescape(regex_group(`<title>(.*?)</title>`, body));
            let hash = regex_group(`<nyaa:infoHash>(.*?)</nyaa:infoHash>`, body);
            if title == "" || hash == "" {
                continue;
            }

            results.push(#{
                title: title,
                magnet_link: "magnet:?xt=urn:btih:" + hash + "&dn=" + url_encode(title) + trackers,
                size: regex_group(`<nyaa:size>(.*?)</nyaa:size>`, body),
                seeds: parse_int_or(regex_group(`<nyaa:seeders>(\d+)</nyaa:seeders>`, body), 0),
                peers: parse_int_or(regex_group(`<nyaa:leechers>(\d+)</nyaa:leechers>`, body), 0)
            });
        }
    }

    results
}
