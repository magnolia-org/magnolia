fn manifest() {
    #{
        name: "ThePirateBay",
        author: "Magnolia",
        type: "tracker",
        tracker_name: "ThePirateBay",
        is_anime: false,
        fields: []
    }
}

fn search(ctx) {
    // cat=200 is the video category
    let url = "https://apibay.org/q.php?q=" + url_encode(ctx.query) + "&cat=200";
    let torrents = json_parse(http_get(url));

    let target_imdb = "";
    if ctx.imdb_id != () {
        target_imdb = ctx.imdb_id;
        target_imdb.replace("tt", "");
    }

    let matches = [];
    let rest = [];
    for t in torrents {
        // Skip the "no results" placeholder entry
        if t.id == "0" || t.name == () || t.name == "" || t.name == "No results returned" {
            continue;
        }
        if t.info_hash == () || t.info_hash == "" {
            continue;
        }

        let result = #{
            title: t.name,
            magnet_link: "magnet:?xt=urn:btih:" + t.info_hash + "&dn=" + url_encode(t.name),
            size: format_size(parse_int_or(t.size, 0)),
            seeds: parse_int_or(t.seeders, 0),
            peers: parse_int_or(t.leechers, 0)
        };

        let result_imdb = "";
        if t.imdb != () {
            result_imdb = t.imdb;
            result_imdb.replace("tt", "");
        }

        if target_imdb != "" && result_imdb == target_imdb {
            matches.push(result);
        } else {
            rest.push(result);
        }
    }

    matches.sort(|a, b| b.seeds - a.seeds);
    rest.sort(|a, b| b.seeds - a.seeds);
    matches + rest
}
