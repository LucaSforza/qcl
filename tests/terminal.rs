#[test]
fn dumb_tty_keeps_linenoise_editing_enabled() {
    let source = include_str!("../vendor/linenoise-rust/native/linenoise.cpp");
    assert!(source.contains("TERM=dumb is still commonly used for real PTYs"));
    assert!(
        source.contains("static const char* unsupported_term[] = {\"cons25\", \"emacs\", NULL};")
    );
}
