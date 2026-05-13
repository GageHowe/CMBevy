use axum::response::Html;

pub(crate) enum Page {
    Assets,
    Lobbies,
}

pub(crate) fn page(title: &str, active: Page, body: &str, script: &str) -> Html<String> {
    Html(format!(
        r#"<!doctype html>
<html lang="en">
    <head>
        <meta charset="UTF-8" />
        <meta name="viewport" content="width=device-width, initial-scale=1.0" />
        <title>{title}</title>
        <link rel="stylesheet" href="/theme.css" />
        <script src="https://unpkg.com/htmx.org@1.9.12"></script>
    </head>
    <body>
        {nav}
        <main>
            {body}
        </main>

        <script>
            {status_script}
            {script}
            checkStatus();
            setInterval(checkStatus, 10000);
        </script>
    </body>
</html>
"#,
        title = title,
        nav = nav(active),
        body = body,
        status_script = include_str!("static/status.js"),
        script = script,
    ))
}

fn nav(active: Page) -> &'static str {
    match active {
        Page::Assets => {
            r#"<nav>
            <div class="row">
                <a class="nav-link active" href="/assets">Assets</a>
                <a class="nav-link" href="/beacon">Lobbies</a>
            </div>
            <span class="status row">
                <span class="status-dot" id="status-dot"></span>
                <span id="status-text">Connecting...</span>
            </span>
        </nav>"#
        }
        Page::Lobbies => {
            r#"<nav>
            <a href="/assets">Beacon</a>
            <div class="row">
                <a class="nav-link" href="/assets">Assets</a>
                <a class="nav-link active" href="/beacon">Lobbies</a>
            </div>
            <span class="status row">
                <span class="status-dot" id="status-dot"></span>
                <span id="status-text">Connecting...</span>
            </span>
        </nav>"#
        }
    }
}
