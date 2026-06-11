fn manifest() {
    #{
        name: "EZTV",
        author: "Magnolia",
        type: "tracker",
        tracker_name: "EZTV",
        is_anime: false,
        fields: []
    }
}

fn search(ctx) {
    if ctx.imdb_id == () || ctx.imdb_id == "" {
        log("EZTV: no IMDB id available, skipping");
        return [];
    }
    let imdb = ctx.imdb_id;
    imdb.replace("tt", "");

    let url = "https://eztvx.to/api/get-torrents?imdb_id=" + imdb + "&limit=100";
    let data = json_parse(http_get(url));
    if data.torrents == () {
        return [];
    }

    let results = [];
    for t in data.torrents {
        if t.title == () || t.magnet_url == () {
            continue;
        }
        let season = parse_int_or(t.season, 0);
        let episode = parse_int_or(t.episode, 0);
        results.push(#{
            title: t.title,
            magnet_link: t.magnet_url,
            size: format_size(parse_int_or(t.size_bytes, 0)),
            seeds: parse_int_or(t.seeds, 0),
            peers: parse_int_or(t.peers, 0),
            season: if season > 0 { season } else { () },
            episode: if episode > 0 { episode } else { () }
        });
    }

    results.sort(|a, b| b.seeds - a.seeds);
    results
}
