use axum::response::Html;

pub(crate) enum Page {
    Home,
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
            {script}
        </script>
    </body>
</html>
"#,
        title = title,
        nav = nav(active),
        body = body,
        script = script,
    ))
}

fn nav(active: Page) -> &'static str {
    match active {
        Page::Home => {
            r#"<nav>
            <a class="brand" href="/">Critical Mass</a>
            <div class="row">
                <a class="nav-link active" href="/">Home</a>
                <a class="nav-link" href="/beacon">Custom Games</a>
                <a class="nav-link" href="/assets">Assets</a>
            </div>
        </nav>"#
        }
        Page::Assets => {
            r#"<nav>
            <a class="brand" href="/">Critical Mass</a>
            <div class="row">
                <a class="nav-link" href="/">Home</a>
                <a class="nav-link" href="/beacon">Custom Games</a>
                <a class="nav-link active" href="/assets">Assets</a>
            </div>
        </nav>"#
        }
        Page::Lobbies => {
            r#"<nav>
            <a class="brand" href="/">Critical Mass</a>
            <div class="row">
                <a class="nav-link" href="/">Home</a>
                <a class="nav-link active" href="/beacon">Custom Games</a>
                <a class="nav-link" href="/assets">Assets</a>
            </div>
        </nav>"#
        }
    }
}
