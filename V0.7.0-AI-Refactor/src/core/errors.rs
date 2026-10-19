fn bad_request(msg: impl Into<String>) -> (StatusCode, String) { (StatusCode::BAD_REQUEST, msg.into()) }
fn internal_error(msg: impl Into<String>) -> (StatusCode, String) { (StatusCode::INTERNAL_SERVER_ERROR, msg.into()) }
fn internal_io(e: std::io::Error) -> (StatusCode, String) { internal_error(format!("檔案操作失敗：{e}")) }
