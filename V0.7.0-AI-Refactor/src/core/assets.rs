async fn index_html() -> Html<&'static str> { Html(include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/web/index.html"))) }
async fn app_js() -> impl IntoResponse {
    ([(header::CONTENT_TYPE, "text/javascript; charset=utf-8")], include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/web/app.js")))
}
async fn style_css() -> impl IntoResponse {
    ([(header::CONTENT_TYPE, "text/css; charset=utf-8")], include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/web/style.css")))
}

async fn ui_v07_js() -> impl IntoResponse {
    ([(header::CONTENT_TYPE, "text/javascript; charset=utf-8")], include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/web/ui-v07.js")))
}
async fn ui_v07_css() -> impl IntoResponse {
    ([(header::CONTENT_TYPE, "text/css; charset=utf-8")], include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/web/ui-v07.css")))
}
