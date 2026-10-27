fn manifest() {
    #{
        name: "Nyaa",
        version: "2.2.0",
        author: "Magnolia",
        type: "tracker",
        tracker_name: "Nyaa",
        is_anime: true,
        fields: []
    }
}

fn pad2(n) {
    if n < 10 { "0" + n } else { "" + n }
}

fn ordinal(n) {
    let suffix = if n % 100 >= 11 && n % 100 <= 13 { "th" }
        else if n % 10 == 1 { "st" }
        else if n % 10 == 2 { "nd" }
        else if n % 10 == 3 { "rd" }
        else { "th" };
    "" + n + suffix
}

// letters, digits and single spaces only, which is what nyaa matches words on. "and" goes
// too, since releases write "panty & stocking" and "panty and stocking" alike
fn clean(name) {
    let out = regex_replace_all(`(?i)[^\p{L}\p{N}]+|\band\b`, name, " ");
    out = regex_replace_all(`\s+`, out, " ");
    out.trim();
    out
}

// nyaa's search takes (a|b) and "quoted phrases", and fansubs name seasons
// every way there is, so match the common spellings at once
fn season_terms(s, extra) {
    if s == 1 {
        return "(S01|S1|\"Season 1\"|batch|complete)";
    }
    let terms = "S" + s + "|S" + pad2(s) + "|\"Season " + s + "\"|\"" + ordinal(s) + " Season\"";
    if extra != "" {
        terms += "|\"" + extra + "\"";
    }
    "(" + terms + ")"
}

// the show's names from anilist (romaji, english), else the tmdb title. each name is its own
// query: quoting them as phrases misses releases that punctuate the name differently
fn names(ctx) {
    if ctx.anime == () {
        return [ctx.title];
    }
    let out = [];
    for t in ctx.anime.titles {
        let c = clean(t);
        if c != "" && out.len() < 2 && !out.contains(c) {
            out.push(c);
        }
    }
    out
}

// what the entry's title adds to the show's ("Shimetsu Kaiyuu Zenpen"), another way
// fansubs name a later season
fn entry_suffix(anime) {
    let franchise = clean(anime.franchise_title);
    let entry = clean(anime.title);
    if entry.len() > franchise.len() && entry.to_lower().starts_with(franchise.to_lower()) {
        let rest = entry.sub_string(franchise.len());
        rest.trim();
        return rest;
    }
    ""
}

fn build_queries(ctx) {
    if ctx.anime != () {
        let a = ctx.anime;
        let queries = [];
        for show in names(ctx) {
            if ctx.kind == "episode" {
                if a.season <= 1 {
                    queries.push(show + " " + pad2(a.episode));
                } else {
                    queries.push(show + " " + season_terms(a.season, entry_suffix(a)) + " " + pad2(a.episode));
                }
                if a.absolute_episode != a.episode {
                    queries.push(show + " " + pad2(a.absolute_episode));
                }
            } else if ctx.kind == "season" {
                queries.push(show + " " + season_terms(a.season, entry_suffix(a)));
            } else {
                queries.push(show);
            }
        }
        return queries;
    }

    let title = ctx.title;
    if ctx.kind == "episode" && ctx.episode != () {
        // fansubs number episodes "- 05" with no season for the first one
        if ctx.season == () || ctx.season <= 1 {
            return [title + " " + pad2(ctx.episode)];
        }
        return [title + " " + season_terms(ctx.season, "") + " " + pad2(ctx.episode)];
    }
    if ctx.kind == "season" && ctx.season != () {
        return [title + " " + season_terms(ctx.season, "")];
    }
    // anime movies rarely carry their year, so the bare name works best
    if ctx.kind == "movie" || ctx.kind == "show" {
        return [title];
    }
    [ctx.query]
}

// rhai passes arguments by value, so the parsed items are returned, not pushed into the caller's array
fn parse_feed(xml, trackers) {
    let results = [];
    for item in regex_capture_all(`(?s)<item>(.*?)</item>`, xml) {
        let body = item[1];
        let title = html_unescape(regex_group(`<title>(.*?)</title>`, body));
        let hash = regex_group(`<nyaa:infoHash>(.*?)</nyaa:infoHash>`, body);
        if title == "" || hash == "" {
            continue;
        }

        // the category says what the subtitles are better than any title does
        let category = regex_group(`<nyaa:categoryId>(.*?)</nyaa:categoryId>`, body);
        if category == "1_1" {
            continue; // music videos
        }
        let subtitles = switch category {
            "1_3" => "foreign",   // non-english-translated
            "1_4" => "none",      // raw
            _ => ()
        };

        let result = #{
            title: title,
            info_hash: hash,
            magnet_link: "magnet:?xt=urn:btih:" + hash + "&dn=" + url_encode(title) + trackers,
            // the item's <link> is the .torrent download, which is instant even when the
            // swarm won't hand out metadata (private-flagged releases)
            torrent_url: regex_group(`<link>(https://nyaa\.si/download/\d+\.torrent)</link>`, body),
            size: regex_group(`<nyaa:size>(.*?)</nyaa:size>`, body),
            seeds: parse_int_or(regex_group(`<nyaa:seeders>(\d+)</nyaa:seeders>`, body), 0),
            peers: parse_int_or(regex_group(`<nyaa:leechers>(\d+)</nyaa:leechers>`, body), 0),
            uploaded: parse_date(regex_group(`<pubDate>(.*?)</pubDate>`, body)),
            trusted: regex_group(`<nyaa:trusted>(.*?)</nyaa:trusted>`, body) == "Yes"
        };
        if subtitles != () {
            result.subtitles = subtitles;
        } else if category == "1_2" {
            // english-translated, but the title tells multi from english
            let meta = parse_release(title);
            if meta.subtitles == () {
                result.subtitles = "english";
            }
        }
        results.push(result);
    }
    results
}

fn search(ctx) {
    let trackers = "&tr=" + url_encode("http://nyaa.tracker.wf:7777/announce")
        + "&tr=" + url_encode("udp://tracker.opentrackr.org:1337/announce")
        + "&tr=" + url_encode("udp://open.stealth.si:80/announce")
        + "&tr=" + url_encode("udp://exodus.desync.com:6969/announce")
        + "&tr=" + url_encode("udp://tracker.torrent.eu.org:451/announce");

    // the rss feed ignores the `p` page parameter (every page returns the
    // same first 75 results), so each query is a single fetch
    let urls = [];
    for q in build_queries(ctx) {
        urls.push("https://nyaa.si/?page=rss&f=0&c=1_0&q=" + url_encode(q) + "&s=seeders&o=desc");
    }

    let seen = [];
    let results = [];
    for xml in http_get_all(urls) {
        for r in parse_feed(xml, trackers) {
            if !seen.contains(r.info_hash) {
                seen.push(r.info_hash);
                results.push(r);
            }
        }
    }
    results
}
