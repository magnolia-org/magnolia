fn manifest() {
    #{
        name: "TorBox",
        version: "1.0.0",
        author: "Magnolia",
        type: "debrid",
        debrid_name: "TorBox",
        fields: [
            #{
                key: "api_key",
                label: "API key",
                type: "text",
                hidden: true,
                placeholder: "Your TorBox API key",
            },
        ]
    }
}

fn list_files(ctx) {
    let id = ensure_torrent(ctx);   // TorBox torrent_id
    let files = wait_for_files(ctx, id);

    let out = [];
    for f in files {
        out.push(#{ id: f.id, name: f.name, size: f.size });
    }
    out
}

fn resolve(ctx) {
    let key = require_key(ctx);
    if ctx.file_id == () {
        throw "TorBox: no file was selected to stream.";
    }

    let id = ensure_torrent(ctx);

    let dl = json_parse(http_get(
        api() + "/torrents/requestdl?token=" + key + "&torrent_id=" + id + "&file_id=" + ctx.file_id,
        #{ "Authorization": "Bearer " + key }
    ));
    if dl.success != true {
        throw "TorBox: failed to get stream link: " + dl.detail;
    }

    #{ url: dl.data }
}


fn require_key(ctx) {
    let key = ctx.fields.api_key;
    if key == "" {
        throw "TorBox API key is not set — add it in Extensions → TorBox.";
    }
    key
}

fn ensure_torrent(ctx) {
    let key = require_key(ctx);
    let headers = #{
        "Authorization": "Bearer " + key,
        "Content-Type": "application/x-www-form-urlencoded",
    };
    let body = "magnet=" + url_encode(ctx.magnet);
    let created = json_parse(http_post(api() + "/torrents/createtorrent", body, headers));
    if created.success != true {
        throw "TorBox: failed to add torrent: " + created.detail;
    }
    created.data.torrent_id
}

fn wait_for_files(ctx, torrent_id) {
    let key = require_key(ctx);
    let auth = #{ "Authorization": "Bearer " + key };

    let files = [];
    let attempts = 0;
    while attempts < 20 {
        let list = json_parse(http_get(api() + "/torrents/mylist?bypass_cache=true&id=" + torrent_id, auth));
        if list.success == true {
            let data = list.data;
            if type_of(data) == "array" {
                data = if data.len() > 0 { data[0] } else { () };
            }
            if data != () {
                let fs = data.files;
                if type_of(fs) == "array" && fs.len() > 0 {
                    files = fs;
                    if data.download_present == true || data.download_finished == true {
                        break;
                    }
                }
            }
        }
        attempts += 1;
        sleep_ms(1500);
    }

    if files.len() == 0 {
        throw "TorBox: torrent isn't ready yet. If it isn't cached, let it finish in your TorBox dashboard and try again.";
    }
    files
}
