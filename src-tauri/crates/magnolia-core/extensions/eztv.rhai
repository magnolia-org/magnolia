fn manifest() {
    #{
        name: "EZTV",
        version: "2.0.0",
        author: "Magnolia",
        type: "tracker",
        tracker_name: "EZTV",
        is_anime: false,
        // shows are looked up by imdb id, which ignores the query, so one pass is enough
        search_kinds: ["show"],
        fields: []
    }
}

fn api(imdb, page) {
    "https://eztvx.to/api/get-torrents?imdb_id=" + imdb + "&limit=100&page=" + page
}

fn search(ctx) {
    if ctx.imdb_id == () || ctx.imdb_id == "" {
        log("EZTV: no IMDB id available, skipping");
        return [];
    }
    let imdb = ctx.imdb_id;
    imdb.replace("tt", "");

    let first = json_parse(http_get(api(imdb, 1)));
    if first.torrents == () {
        return [];
    }
    let pages = [first];

    // long shows run past one page; fetch the rest at once
    let total = parse_int_or(first.torrents_count, 0);
    let page_count = (total + 99) / 100;
    if page_count > 5 {
        page_count = 5;
    }
    if page_count > 1 {
        let urls = [];
        for p in 2..=page_count {
            urls.push(api(imdb, p));
        }
        for body in http_get_all(urls) {
            if body != "" {
                pages.push(json_parse(body));
            }
        }
    }

    let results = [];
    for data in pages {
        if data.torrents == () {
            continue;
        }
        for t in data.torrents {
            if t.title == () || t.magnet_url == () {
                continue;
            }
            let season = parse_int_or(t.season, 0);
            let episode = parse_int_or(t.episode, 0);
            // other seasons are noise for the episode being played
            if ctx.season != () && season > 0 && season != ctx.season {
                continue;
            }
            results.push(#{
                title: t.title,
                magnet_link: t.magnet_url,
                info_hash: t.hash,
                size_bytes: parse_int_or(t.size_bytes, 0),
                seeds: parse_int_or(t.seeds, 0),
                peers: parse_int_or(t.peers, 0),
                uploaded: parse_int_or(t.date_released_unix, 0),
                season: if season > 0 { season } else { () },
                episode: if episode > 0 { episode } else { () }
            });
        }
    }

    results.sort(|a, b| b.seeds - a.seeds);
    results
}
