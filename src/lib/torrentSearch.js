import { invoke, Channel } from "@tauri-apps/api/core";

export function resultKey(result) {
    return result.info_hash || result.magnet_link;
}

// runs a streaming torrent search. onUpdate gets { query, mediaType, jobs,
// results, done } at most once a frame while trackers answer. the backend
// cancels whatever search was running before this one.
export function startTorrentSearch(request, onUpdate) {
    const state = {
        searchId: null,
        query: "",
        mediaType: "",
        note: "",
        anime: null,
        jobs: [],
        results: new Map(),
        done: false,
    };

    let scheduled = false;
    const flush = () => {
        scheduled = false;
        onUpdate({
            query: state.query,
            mediaType: state.mediaType,
            note: state.note,
            anime: state.anime,
            jobs: state.jobs.slice(),
            results: [...state.results.values()],
            done: state.done,
        });
    };
    const schedule = () => {
        if (scheduled) return;
        scheduled = true;
        requestAnimationFrame(flush);
    };

    const addJobs = (jobs) => {
        for (const job of jobs) state.jobs.push({ ...job, status: "running", count: 0, error: null });
    };

    const channel = new Channel();
    channel.onmessage = (event) => {
        if (state.searchId !== null && event.search_id !== state.searchId) return;
        switch (event.type) {
            case "started":
                state.searchId = event.search_id;
                state.query = event.query;
                state.mediaType = event.media_type;
                state.anime = event.anime;
                addJobs(event.jobs);
                break;
            case "jobs_added":
                state.note = event.reason;
                addJobs(event.jobs);
                break;
            case "results":
                for (const result of event.results) state.results.set(resultKey(result), result);
                break;
            case "job_finished": {
                const { job } = event;
                const entry = state.jobs.find(j =>
                    j.status === "running" && j.tracker_id === job.tracker_id && j.kind === job.kind && j.query === job.query);
                if (entry) {
                    entry.status = event.error ? "failed" : "done";
                    entry.count = event.count;
                    entry.error = event.error;
                    entry.cached = event.cached;
                    entry.elapsed_ms = event.elapsed_ms;
                }
                break;
            }
            case "finished":
                state.done = true;
                break;
        }
        schedule();
    };

    const promise = invoke("search_torrents", { request, onEvent: channel }).then((response) => {
        // the final list is ranked and merged, so it replaces what streamed in
        state.results = new Map(response.results.map(r => [resultKey(r), r]));
        state.query = response.query;
        state.mediaType = response.media_type;
        state.done = true;
        flush();
        return response;
    });

    return {
        promise,
        cancel: () => invoke("cancel_torrent_search").catch(() => {}),
    };
}
