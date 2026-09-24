// exe에 아이콘 리소스 포함 (리소스 번호 1)
fn main() {
    embed_resource::compile("assets/app.rc", embed_resource::NONE).manifest_optional().unwrap();
}
