use super::*;

static ZIPPY_CFG: &str = "(defsrc lalt)(deflayer base (caps-word 2000))(defzippy file)";
static ZIPPY_FILE_CONTENT: &str = "
dy	day
dy 1	Monday
 abc	Alphabet
pr	pre ⌫
pra	partner
pr q	pull request
r df	recipient
 w  a	Washington
xy	WxYz
rq	request
rqa	request␣assistance
.g	git
.g f p	git fetch -p
12	hi
1234	bye
";

fn simulate_with_zippy_file_content(cfg: &str, input: &str, content: &str) -> String {
    let mut fcontent = FxHashMap::default();
    fcontent.insert("file".into(), content.into());
    simulate_with_file_content(cfg, input, fcontent)
}

// Over-eager chord expansion (RED — open bug). When the keys of one chord are
// pressed and an *independent* chord whose keys are a proper subset of them is
// transiently held, the sub-chord fires eagerly and its expansion is shown, then
// thrown away as the larger chord completes. Real-world repro: chords `er`->"error",
// `res`->"response", `sure`->"sure"; pressing the keys for `sure` in the order
// e,r,s,u (as home-row mods can reorder them) flashes "error" then "response" before
// "sure". The net text is correct ("sure"), so the net-text oracle is blind; the
// churn is the bug.
//
// Intended behaviour, asserted here so the test fails until it is fixed: forming
// `sure` types only "sure" — never the letter `o`, which appears only in the
// discarded "error"/"response" expansions. (Mirrors the PBT `no_overeager_expansion`
// invariant.)
#[test]
fn sim_zippychord_no_overeager_expansion() {
    let content = "er\terror\nres\tresponse\nsure\tsure\n";
    let result = simulate_with_zippy_file_content(ZIPPY_CFG, "d:e d:r d:s d:u t:10 t:300", content)
        .to_ascii();
    assert!(
        !result.contains("dn:O"),
        "over-eager expansion: forming `sure` flashed an intermediate expansion \
         (`error`/`response`) — the letter `o` was typed though `sure` has none.\n  raw: {result}"
    );
}

#[test]
fn sim_zippychord_capitalize() {
    let result = simulate_with_zippy_file_content(
        ZIPPY_CFG,
        "d:a t:10 d:b t:10 d:spc t:10 d:c u:a u:b u:c u:spc t:300 \
         d:a t:10 d:b t:10 d:spc t:10 d:c t:300",
        ZIPPY_FILE_CONTENT,
    )
    .to_ascii();
    assert_eq!(
        "dn:A t:10ms dn:B t:10ms dn:Space t:10ms \
         dn:BSpace up:BSpace dn:BSpace up:BSpace dn:BSpace up:BSpace \
         dn:LShift up:A dn:A up:LShift \
         dn:L up:L dn:P up:P dn:H up:H up:A dn:A up:B dn:B dn:E up:E dn:T up:T \
         t:1ms up:A t:1ms up:B t:1ms up:C t:1ms up:Space t:296ms \
         dn:A t:10ms dn:B t:10ms dn:Space t:10ms \
         dn:BSpace up:BSpace dn:BSpace up:BSpace dn:BSpace up:BSpace \
         dn:LShift up:A dn:A up:LShift \
         dn:L up:L dn:P up:P dn:H up:H up:A dn:A up:B dn:B dn:E up:E dn:T up:T",
        result
    );
}

#[test]
fn sim_zippychord_followup_with_prev() {
    let result = simulate_with_zippy_file_content(
        ZIPPY_CFG,
        "d:d t:10 d:y t:10 u:d u:y t:10 d:1 t:300",
        ZIPPY_FILE_CONTENT,
    )
    .to_ascii();
    assert_eq!(
        "dn:D t:10ms dn:A up:A up:Y dn:Y t:10ms up:D t:1ms up:Y t:9ms dn:BSpace up:BSpace dn:BSpace up:BSpace dn:BSpace up:BSpace dn:LShift dn:M up:M up:LShift dn:O up:O dn:N up:N dn:D up:D dn:A up:A dn:Y up:Y",
        result
    );
}

#[test]
fn sim_zippychord_followup_no_prev() {
    let result = simulate_with_zippy_file_content(
        ZIPPY_CFG,
        "d:r t:10 u:r t:10 d:d d:f t:10 t:300",
        ZIPPY_FILE_CONTENT,
    )
    .to_ascii();
    assert_eq!(
        "dn:R t:10ms up:R t:10ms dn:D t:1ms \
        dn:BSpace up:BSpace \
        dn:E up:E dn:C up:C dn:I up:I dn:P up:P dn:I up:I dn:E up:E dn:N up:N dn:T up:T",
        result
    );
}

#[test]
fn sim_zippychord_washington() {
    let result = simulate_with_zippy_file_content(
        ZIPPY_CFG,
        "d:w d:spc t:10
         u:w u:spc t:10
         d:a d:spc t:10
         u:a u:spc t:300",
        ZIPPY_FILE_CONTENT,
    )
    .to_ascii();
    assert_eq!(
        "dn:W t:1ms dn:Space t:9ms up:W t:1ms up:Space t:9ms \
         dn:A t:1ms dn:BSpace up:BSpace dn:BSpace up:BSpace dn:BSpace up:BSpace \
         dn:LShift dn:W up:W up:LShift \
         up:A dn:A dn:S up:S dn:H up:H dn:I up:I dn:N up:N dn:G up:G dn:T up:T dn:O up:O dn:N up:N \
         t:9ms up:A t:1ms up:Space",
        result
    );
}

#[test]
fn sim_zippychord_overlap() {
    let result = simulate_with_zippy_file_content(
        ZIPPY_CFG,
        "d:r t:10  d:q t:10 d:a t:10",
        ZIPPY_FILE_CONTENT,
    )
    .to_ascii();
    assert_eq!(
        "dn:R t:10ms dn:Q t:10ms dn:BSpace up:BSpace dn:E up:E up:Q dn:Q dn:U up:U dn:E up:E dn:S up:S dn:T up:T dn:Space up:Space up:A dn:A dn:S up:S dn:S up:S dn:I up:I dn:S up:S dn:T up:T up:A dn:A dn:N up:N dn:C up:C dn:E up:E",
        result
    );
    let result =
        simulate_with_zippy_file_content(ZIPPY_CFG, "d:1 d:2 d:3 d:4 t:20", ZIPPY_FILE_CONTENT)
            .to_ascii();
    assert_eq!(
        "dn:Kb1 t:1ms dn:Kb2 t:1ms dn:Kb3 t:1ms dn:BSpace up:BSpace dn:BSpace up:BSpace dn:BSpace up:BSpace dn:B up:B dn:Y up:Y dn:E up:E",
        result
    );
}

#[test]
fn sim_zippychord_lsft() {
    // test lsft behaviour while pressed
    let result = simulate_with_zippy_file_content(
        ZIPPY_CFG,
        "d:lsft t:10 d:d t:10 d:y t:10",
        ZIPPY_FILE_CONTENT,
    )
    .to_ascii();
    assert_eq!(
        "dn:LShift t:10ms dn:D t:10ms dn:BSpace up:BSpace up:D dn:D up:LShift dn:A up:A up:Y dn:Y dn:LShift",
        result
    );
    let result = simulate_with_zippy_file_content(
        ZIPPY_CFG,
        "d:lsft t:10 d:x t:10 d:y t:10",
        ZIPPY_FILE_CONTENT,
    )
    .to_ascii();
    assert_eq!(
        "dn:LShift t:10ms dn:X t:10ms dn:BSpace up:BSpace \
         dn:W up:W up:LShift up:X dn:X dn:LShift up:Y dn:Y up:LShift dn:Z up:Z dn:LShift",
        result
    );

    // ensure lsft-held behaviour goes away when released
    let result = simulate_with_zippy_file_content(
        ZIPPY_CFG,
        "d:lsft t:10 d:d u:lsft t:10 d:y t:10",
        ZIPPY_FILE_CONTENT,
    )
    .to_ascii();
    assert_eq!(
        "dn:LShift t:10ms dn:D t:1ms up:LShift t:9ms dn:BSpace up:BSpace up:D dn:D dn:A up:A up:Y dn:Y",
        result
    );
    let result = simulate_with_zippy_file_content(
        ZIPPY_CFG,
        "d:lsft t:10 d:x u:lsft t:10 d:y t:10",
        ZIPPY_FILE_CONTENT,
    )
    .to_ascii();
    assert_eq!(
        "dn:LShift t:10ms dn:X t:1ms up:LShift t:9ms dn:BSpace up:BSpace \
         dn:LShift dn:W up:W up:LShift up:X dn:X dn:LShift up:Y dn:Y up:LShift dn:Z up:Z",
        result
    );
}

#[test]
fn sim_zippychord_rsft() {
    // test rsft behaviour while pressed
    let result = simulate_with_zippy_file_content(
        ZIPPY_CFG,
        "d:rsft t:10 d:d t:10 d:y t:10",
        ZIPPY_FILE_CONTENT,
    )
    .to_ascii();
    assert_eq!(
        "dn:RShift t:10ms dn:D t:10ms dn:BSpace up:BSpace up:D dn:D up:RShift dn:A up:A up:Y dn:Y dn:RShift",
        result
    );
    let result = simulate_with_zippy_file_content(
        ZIPPY_CFG,
        "d:rsft t:10 d:x t:10 d:y t:10",
        ZIPPY_FILE_CONTENT,
    )
    .to_ascii();
    assert_eq!(
        "dn:RShift t:10ms dn:X t:10ms dn:BSpace up:BSpace \
         dn:W up:W up:RShift up:X dn:X dn:LShift up:Y dn:Y up:LShift dn:Z up:Z dn:RShift",
        result
    );

    // ensure rsft-held behaviour goes away when released
    let result = simulate_with_zippy_file_content(
        ZIPPY_CFG,
        "d:rsft t:10 d:d u:rsft t:10 d:y t:10",
        ZIPPY_FILE_CONTENT,
    )
    .to_ascii();
    assert_eq!(
        "dn:RShift t:10ms dn:D t:1ms up:RShift t:9ms dn:BSpace up:BSpace up:D dn:D dn:A up:A up:Y dn:Y",
        result
    );
    let result = simulate_with_zippy_file_content(
        ZIPPY_CFG,
        "d:rsft t:10 d:x u:rsft t:10 d:y t:10",
        ZIPPY_FILE_CONTENT,
    )
    .to_ascii();
    assert_eq!(
        "dn:RShift t:10ms dn:X t:1ms up:RShift t:9ms dn:BSpace up:BSpace \
         dn:LShift dn:W up:W up:LShift up:X dn:X dn:LShift up:Y dn:Y up:LShift dn:Z up:Z",
        result
    );
}

#[test]
fn sim_zippychord_ralt() {
    // test ralt behaviour while pressed
    let result = simulate_with_zippy_file_content(
        ZIPPY_CFG,
        "d:ralt t:10 d:d t:10 d:y t:10",
        ZIPPY_FILE_CONTENT,
    )
    .to_ascii();
    assert_eq!(
        "dn:RAlt t:10ms dn:D t:10ms dn:BSpace up:BSpace up:RAlt up:D dn:D dn:A up:A up:Y dn:Y dn:RAlt",
        result
    );
    let result = simulate_with_zippy_file_content(
        ZIPPY_CFG,
        "d:ralt t:10 d:x t:10 d:y t:10",
        ZIPPY_FILE_CONTENT,
    )
    .to_ascii();
    assert_eq!(
        "dn:RAlt t:10ms dn:X t:10ms dn:BSpace up:BSpace \
         up:RAlt dn:LShift dn:W up:W up:LShift up:X dn:X dn:LShift up:Y dn:Y up:LShift dn:Z up:Z dn:RAlt",
        result
    );

    // ensure rsft-held behaviour goes away when released
    let result = simulate_with_zippy_file_content(
        ZIPPY_CFG,
        "d:ralt t:10 d:d u:ralt t:10 d:y t:10",
        ZIPPY_FILE_CONTENT,
    )
    .to_ascii();
    assert_eq!(
        "dn:RAlt t:10ms dn:D t:1ms up:RAlt t:9ms dn:BSpace up:BSpace up:D dn:D dn:A up:A up:Y dn:Y",
        result
    );
    let result = simulate_with_zippy_file_content(
        ZIPPY_CFG,
        "d:ralt t:10 d:x u:ralt t:10 d:y t:10",
        ZIPPY_FILE_CONTENT,
    )
    .to_ascii();
    assert_eq!(
        "dn:RAlt t:10ms dn:X t:1ms up:RAlt t:9ms dn:BSpace up:BSpace \
         dn:LShift dn:W up:W up:LShift up:X dn:X dn:LShift up:Y dn:Y up:LShift dn:Z up:Z",
        result
    );
}

#[test]
fn sim_zippychord_caps_word() {
    let result = simulate_with_zippy_file_content(
        ZIPPY_CFG,
        "d:lalt u:lalt t:10 d:d t:10 d:y t:10 u:d u:y t:10 d:spc u:spc t:2000 d:d d:y t:10",
        ZIPPY_FILE_CONTENT,
    )
    .to_ascii();
    assert_eq!(
        "t:10ms dn:LShift dn:D t:10ms dn:BSpace up:BSpace up:D dn:D dn:A up:A up:Y dn:Y t:10ms up:D t:1ms up:LShift up:Y t:9ms dn:Space t:1ms up:Space t:1999ms dn:D t:1ms dn:A up:A up:Y dn:Y",
        result
    );
    let result = simulate_with_zippy_file_content(
        ZIPPY_CFG,
        "d:lalt t:10 d:y t:10 d:x t:10 u:x u:y t:10 d:spc u:spc t:1000 d:y d:x t:10",
        ZIPPY_FILE_CONTENT,
    )
    .to_ascii();
    assert_eq!(
        "t:10ms dn:LShift dn:Y t:10ms dn:BSpace up:BSpace \
         dn:W up:W up:X dn:X up:Y dn:Y dn:Z up:Z \
         t:10ms up:X t:1ms up:LShift up:Y t:9ms dn:Space t:1ms up:Space \
         t:999ms dn:Y t:1ms dn:BSpace up:BSpace dn:LShift dn:W up:W up:LShift \
         up:X dn:X dn:LShift up:Y dn:Y up:LShift dn:Z up:Z",
        result
    );
}

#[test]
fn sim_zippychord_triple_combo() {
    let result = simulate_with_zippy_file_content(
        ZIPPY_CFG,
        "d:. d:g t:10 u:. u:g d:f t:10 u:f d:p t:10",
        ZIPPY_FILE_CONTENT,
    )
    .to_ascii();
    assert_eq!(
        "dn:Dot t:1ms dn:BSpace up:BSpace up:G dn:G dn:I up:I dn:T up:T t:9ms up:Dot t:1ms up:G \
         t:1ms dn:F t:8ms up:F t:1ms \
         dn:BSpace up:BSpace dn:Space up:Space \
         dn:F up:F dn:E up:E dn:T up:T dn:C up:C dn:H up:H dn:Space up:Space \
         dn:Minus up:Minus up:P dn:P",
        result
    );
}

#[test]
fn sim_zippychord_disabled_by_typing() {
    let result = simulate_with_zippy_file_content(
        ZIPPY_CFG,
        "d:v u:v t:10 d:d d:y t:100",
        ZIPPY_FILE_CONTENT,
    )
    .to_ascii();
    assert_eq!("dn:V t:1ms up:V t:9ms dn:D t:1ms dn:Y", result);
}

#[test]
fn sim_zippychord_prefix() {
    let result = simulate_with_zippy_file_content(
        ZIPPY_CFG,
        "d:p d:r u:p u:r t:10 d:q u:q t:10",
        ZIPPY_FILE_CONTENT,
    )
    .to_ascii();
    assert_eq!(
        "dn:P t:1ms dn:R t:1ms dn:E up:E dn:Space up:Space dn:BSpace up:BSpace up:P t:1ms up:R t:7ms dn:BSpace up:BSpace dn:BSpace up:BSpace dn:U up:U dn:L up:L dn:L up:L dn:Space up:Space dn:R up:R dn:E up:E up:Q dn:Q dn:U up:U dn:E up:E dn:S up:S dn:T up:T t:1ms up:Q",
        result
    );
    let result = simulate_with_zippy_file_content(
        ZIPPY_CFG,
        "d:p d:r d:a t:10 u:d u:r u:a",
        ZIPPY_FILE_CONTENT,
    )
    .to_ascii()
    .no_time()
    .no_releases();
    assert_eq!("dn:P dn:R dn:BSpace dn:A dn:R dn:T dn:N dn:E dn:R", result);
}

#[test]
fn sim_zippychord_smartspace_full() {
    let result = simulate_with_zippy_file_content(
        "(defsrc)(deflayer base)(defzippy file
         smart-space full)",
        "d:d d:y t:10 u:d u:y t:100 d:. t:10 u:. t:10",
        ZIPPY_FILE_CONTENT,
    )
    .to_ascii();
    assert_eq!(
        "dn:D t:1ms dn:A up:A up:Y dn:Y dn:Space up:Space t:9ms up:D t:1ms up:Y t:99ms dn:BSpace up:BSpace dn:Dot t:10ms up:Dot",
        result
    );

    // Test that prefix works as intended.
    let result = simulate_with_zippy_file_content(
        "(defsrc)(deflayer base)(defzippy file
         smart-space add-space-only)",
        "d:p d:r t:10 u:p u:r t:100 d:. t:10 u:. t:10",
        ZIPPY_FILE_CONTENT,
    )
    .to_ascii();
    assert_eq!(
        "dn:P t:1ms dn:R t:9ms dn:E up:E dn:Space up:Space dn:BSpace up:BSpace up:P t:1ms up:R t:99ms dn:Dot t:10ms up:Dot",
        result
    );
}

#[test]
fn sim_zippychord_smartspace_spaceonly() {
    let result = simulate_with_zippy_file_content(
        "(defsrc)(deflayer base)(defzippy file
         smart-space add-space-only)",
        "d:d d:y t:10 u:d u:y t:100 d:. t:10 u:. t:10",
        ZIPPY_FILE_CONTENT,
    )
    .to_ascii();
    assert_eq!(
        "dn:D t:1ms dn:A up:A up:Y dn:Y dn:Space up:Space t:9ms up:D t:1ms up:Y t:99ms dn:Dot t:10ms up:Dot",
        result
    );

    // Test that prefix works as intended.
    let result = simulate_with_zippy_file_content(
        "(defsrc)(deflayer base)(defzippy file
         smart-space add-space-only)",
        "d:p d:r t:10 u:p u:r t:100 d:. t:10 u:. t:10",
        ZIPPY_FILE_CONTENT,
    )
    .to_ascii();
    assert_eq!(
        "dn:P t:1ms dn:R t:9ms dn:E up:E dn:Space up:Space dn:BSpace up:BSpace up:P t:1ms up:R t:99ms dn:Dot t:10ms up:Dot",
        result
    );
}

#[test]
fn sim_zippychord_smartspace_none() {
    let result = simulate_with_zippy_file_content(
        "(defsrc)(deflayer base)(defzippy file
         smart-space none)",
        "d:d d:y t:10 u:d u:y t:100 d:. t:10 u:. t:10",
        ZIPPY_FILE_CONTENT,
    )
    .to_ascii();
    assert_eq!(
        "dn:D t:1ms dn:A up:A up:Y dn:Y t:9ms up:D t:1ms up:Y t:99ms dn:Dot t:10ms up:Dot",
        result
    );

    // Test that prefix works as intended.
    let result = simulate_with_zippy_file_content(
        "(defsrc)(deflayer base)(defzippy file
         smart-space add-space-only)",
        "d:p d:r t:10 u:p u:r t:100 d:. t:10 u:. t:10",
        ZIPPY_FILE_CONTENT,
    )
    .to_ascii();
    assert_eq!(
        "dn:P t:1ms dn:R t:9ms dn:E up:E dn:Space up:Space dn:BSpace up:BSpace up:P t:1ms up:R t:99ms dn:Dot t:10ms up:Dot",
        result
    );
}

#[test]
fn sim_zippychord_smartspace_overlap() {
    let result = simulate_with_zippy_file_content(
        "(defsrc)(deflayer base)(defzippy file
         smart-space full)",
        "d:r t:10 d:q t:10 d:a t:10",
        ZIPPY_FILE_CONTENT,
    )
    .to_ascii();
    assert_eq!(
        "dn:R t:10ms dn:Q t:10ms dn:BSpace up:BSpace dn:E up:E up:Q dn:Q dn:U up:U dn:E up:E dn:S up:S dn:T up:T dn:Space up:Space up:A dn:A dn:S up:S dn:S up:S dn:I up:I dn:S up:S dn:T up:T up:A dn:A dn:N up:N dn:C up:C dn:E up:E dn:Space up:Space",
        result
    );
    let result = simulate_with_zippy_file_content(
        "(defsrc)(deflayer base)(defzippy file
         smart-space full)",
        "d:1 d:2 d:3 d:4 t:20",
        ZIPPY_FILE_CONTENT,
    )
    .to_ascii();
    assert_eq!(
        "dn:Kb1 t:1ms dn:Kb2 t:1ms dn:Kb3 t:1ms dn:BSpace up:BSpace dn:BSpace up:BSpace dn:BSpace up:BSpace dn:B up:B dn:Y up:Y dn:E up:E dn:Space up:Space",
        result
    );
}

#[test]
fn sim_zippychord_smartspace_followup() {
    let result = simulate_with_zippy_file_content(
        "(defsrc)(deflayer base)(defzippy file
         smart-space full)",
        "d:d t:10 d:y t:10 u:d u:y t:10 d:1 t:300",
        ZIPPY_FILE_CONTENT,
    )
    .to_ascii();
    assert_eq!(
        "dn:D t:10ms dn:A up:A up:Y dn:Y dn:Space up:Space t:10ms up:D t:1ms up:Y t:9ms dn:BSpace up:BSpace dn:BSpace up:BSpace dn:BSpace up:BSpace dn:BSpace up:BSpace dn:LShift dn:M up:M up:LShift dn:O up:O dn:N up:N dn:D up:D dn:A up:A dn:Y up:Y dn:Space up:Space",
        result
    );
}

const CUSTOM_PUNC_CFG: &str = "\
(defsrc)
(deflayer base)
(defzippy file
 smart-space full
 smart-space-punctuation (z ! ® *)
 output-character-mappings (
   ® AG-r
   * S-AG-v
   ! S-1))";

#[test]
fn sim_zippychord_smartspace_custom_punc() {
    // 1 without lsft: no smart-space-erase
    let result = simulate_with_zippy_file_content(
        CUSTOM_PUNC_CFG,
        "d:d t:10 d:y t:10 u:d u:y t:10 d:1 t:300",
        ZIPPY_FILE_CONTENT,
    )
    .to_ascii();
    assert_eq!(
        "dn:D t:10ms dn:A up:A up:Y dn:Y dn:Space up:Space t:10ms up:D t:1ms up:Y t:9ms dn:BSpace up:BSpace dn:BSpace up:BSpace dn:BSpace up:BSpace dn:BSpace up:BSpace dn:LShift dn:M up:M up:LShift dn:O up:O dn:N up:N dn:D up:D dn:A up:A dn:Y up:Y dn:Space up:Space",
        result
    );

    // S-1 = !: smart-space-erase
    let result = simulate_with_zippy_file_content(
        CUSTOM_PUNC_CFG,
        "d:1 d:2 t:10 u:1 u:2 t:10 d:lsft d:1 u:1 u:lsft t:300",
        ZIPPY_FILE_CONTENT,
    )
    .to_ascii();
    assert_eq!(
        "dn:Kb1 t:1ms dn:Kb2 t:9ms dn:BSpace up:BSpace dn:BSpace up:BSpace dn:H up:H dn:I up:I dn:Space up:Space up:Kb1 t:1ms up:Kb2 t:9ms dn:LShift t:1ms dn:BSpace up:BSpace dn:Kb1 t:1ms up:Kb1 t:1ms up:LShift",
        result
    );

    // z: smart-space-erase
    let result = simulate_with_zippy_file_content(
        CUSTOM_PUNC_CFG,
        "d:1 d:2 t:10 u:1 u:2 t:10 d:z u:z t:300",
        ZIPPY_FILE_CONTENT,
    )
    .to_ascii();
    assert_eq!(
        "dn:Kb1 t:1ms dn:Kb2 t:9ms dn:BSpace up:BSpace dn:BSpace up:BSpace dn:H up:H dn:I up:I dn:Space up:Space up:Kb1 t:1ms up:Kb2 t:9ms dn:BSpace up:BSpace dn:Z t:1ms up:Z",
        result
    );

    // r no altgr: no smart-space-erase
    let result = simulate_with_zippy_file_content(
        CUSTOM_PUNC_CFG,
        "d:1 d:2 t:10 u:1 u:2 t:10 d:r u:r t:300",
        ZIPPY_FILE_CONTENT,
    )
    .to_ascii();
    assert_eq!(
        "dn:Kb1 t:1ms dn:Kb2 t:9ms dn:BSpace up:BSpace dn:BSpace up:BSpace dn:H up:H dn:I up:I dn:Space up:Space up:Kb1 t:1ms up:Kb2 t:9ms dn:R t:1ms up:R",
        result
    );

    // r with altgr: smart-space-erase
    let result = simulate_with_zippy_file_content(
        CUSTOM_PUNC_CFG,
        "d:1 d:2 t:10 u:1 u:2 t:10 d:ralt d:r u:r u:ralt t:300",
        ZIPPY_FILE_CONTENT,
    )
    .to_ascii();
    assert_eq!(
        "dn:Kb1 t:1ms dn:Kb2 t:9ms dn:BSpace up:BSpace dn:BSpace up:BSpace dn:H up:H dn:I up:I dn:Space up:Space up:Kb1 t:1ms up:Kb2 t:9ms dn:RAlt t:1ms dn:BSpace up:BSpace dn:R t:1ms up:R t:1ms up:RAlt",
        result
    );

    // v with altgr+lsft: smart-space-erase
    let result = simulate_with_zippy_file_content(
        CUSTOM_PUNC_CFG,
        "d:1 d:2 t:10 u:1 u:2 t:10 d:ralt d:lsft d:v u:v u:ralt u:lsft t:300",
        ZIPPY_FILE_CONTENT,
    )
    .to_ascii();
    assert_eq!(
        "dn:Kb1 t:1ms dn:Kb2 t:9ms dn:BSpace up:BSpace dn:BSpace up:BSpace dn:H up:H dn:I up:I dn:Space up:Space up:Kb1 t:1ms up:Kb2 t:9ms dn:RAlt t:1ms dn:LShift t:1ms dn:BSpace up:BSpace dn:V t:1ms up:V t:1ms up:RAlt t:1ms up:LShift",
        result
    );
}

#[test]
fn sim_zippychord_non_followup_subsequent_with_potential_followups_available() {
    let result = simulate_with_zippy_file_content(
        "(defsrc)(deflayer base)(defzippy file
         smart-space full)",
        "d:g d:. t:10 u:g u:. t:1000 d:g d:. t:10 u:g u:. t:1000",
        ZIPPY_FILE_CONTENT,
    )
    .to_ascii();
    assert_eq!(
        "dn:G t:1ms dn:I up:I dn:T up:T dn:Space up:Space t:9ms up:G t:1ms up:Dot t:999ms dn:G t:1ms dn:I up:I dn:T up:T dn:Space up:Space t:9ms up:G t:1ms up:Dot",
        result
    );
}

const DEAD_KEYS_CFG: &str = "\
(defsrc)
(deflayer base)
(defzippy file
 smart-space full
 output-character-mappings (
   ’ (no-erase ')
   ‘ (no-erase `)
   é (single-output ' e)
   è (single-output ` e)
 ))";
static DEAD_KEYS_FILE_CONTENT: &str = "
by	h’elo
bye	by‘e
by d	ft‘a’ng
by d a	aye
cy	hélo
cye	byè
cy d	ftéèng
cy d a	aye
";

#[test]
fn sim_zippychord_noerase() {
    let result = simulate_with_zippy_file_content(
        DEAD_KEYS_CFG,
        "d:b d:y t:100 d:e u:b u:y u:e t:1000",
        DEAD_KEYS_FILE_CONTENT,
    )
    .no_releases()
    .no_time()
    .to_ascii();
    assert_eq!(
        "dn:B dn:BSpace dn:H dn:Quote dn:E dn:L dn:O dn:Space \
         dn:BSpace dn:BSpace dn:BSpace dn:BSpace dn:BSpace \
         dn:B dn:Y dn:Grave dn:E dn:Space",
        result,
    );

    let result = simulate_with_zippy_file_content(
        DEAD_KEYS_CFG,
        "d:b d:y t:100 u:b u:y d:d t:10 u:d d:a t:10 u:a t:1000",
        DEAD_KEYS_FILE_CONTENT,
    )
    .no_releases()
    .no_time()
    .to_ascii();
    assert_eq!(
        "dn:B dn:BSpace dn:H dn:Quote dn:E dn:L dn:O dn:Space \
         dn:BSpace dn:BSpace dn:BSpace dn:BSpace dn:BSpace \
         dn:F dn:T dn:Grave dn:A dn:Quote dn:N dn:G dn:Space \
         dn:BSpace dn:BSpace dn:BSpace dn:BSpace dn:BSpace dn:BSpace \
         dn:A dn:Y dn:E dn:Space",
        result,
    );
}

#[test]
fn sim_zippychord_single_output() {
    let result = simulate_with_zippy_file_content(
        DEAD_KEYS_CFG,
        "d:c d:y t:100 d:e u:c u:y u:e t:1000",
        DEAD_KEYS_FILE_CONTENT,
    )
    .no_releases()
    .no_time()
    .to_ascii();
    assert_eq!(
        "dn:C dn:BSpace dn:H dn:Quote dn:E dn:L dn:O dn:Space \
         dn:BSpace dn:BSpace dn:BSpace dn:BSpace dn:BSpace \
         dn:B dn:Y dn:Grave dn:E dn:Space",
        result,
    );

    let result = simulate_with_zippy_file_content(
        DEAD_KEYS_CFG,
        "d:c d:y t:100 u:c u:y d:d t:10 u:d d:a t:10 u:a t:1000",
        DEAD_KEYS_FILE_CONTENT,
    )
    .no_releases()
    .no_time()
    .to_ascii();
    assert_eq!(
        "dn:C dn:BSpace dn:H dn:Quote dn:E dn:L dn:O dn:Space \
         dn:BSpace dn:BSpace dn:BSpace dn:BSpace dn:BSpace \
         dn:F dn:T dn:Quote dn:E dn:Grave dn:E dn:N dn:G dn:Space \
         dn:BSpace dn:BSpace dn:BSpace dn:BSpace dn:BSpace dn:BSpace dn:BSpace \
         dn:A dn:Y dn:E dn:Space",
        result,
    );
}

// --- Leading-space chord non-determinism reproduction --------------------
//
// A leading space in the zippy input means SPACE is a participating chord key
// (e.g. " a" is the chord SPACE+a). Note the explicit \n and leading spaces:
// do NOT rewrite this with a "\" line-continuation, that would strip the
// significant leading spaces and silently change the chords.
static ZIPPY_LEADING_SPACE_CONTENT: &str = "\n a\ta\n das\tdass\ndas\tdas\n";

// User config: short chord deadline, as in the real setup.
static ZIPPY_CFG_DEADLINE50: &str =
    "(defsrc lalt)(deflayer base (caps-word 2000))(defzippy file on-first-press-chord-deadline 50)";

// Press-order-dependent zippychord bug that the coupled state-machine PBT
// (`kanata_proptest.rs`) CANNOT catch, because its zippy + tap-hold keys are
// drawn from disjoint alphabets — no key is ever both a chord participant and a
// tap-hold action, which is the precondition for this bug. (The PBT's
// `interaction_taphold_zippy_order_independent` IS the overlap reproduction; this
// is its deterministic sibling.) The bug needs a chord-participating key that is a
// `tap-hold` (or layer) key: the layout then *delays* that key's tap output by an
// order/timing-dependent amount.
//
// Here SPACE is `(tap-hold 200 200 spc ...)` while the chord deadline is 20 ticks
// (mirrors the reported real config: space is a 200ms tap-hold thumb key, deadline
// 50). The chord is " n" -> "no". The two press orders are the SAME physical
// gesture (a chord is a set) and MUST produce the same visible text "no ":
//
//   - 'n' first: 'n' starts the 20-tick chord deadline; the space tap-hold has not
//     resolved by the time it expires, so zippychord disables and space arrives as
//     a literal -> "n " (chord never fires). <-- BUG.
//   - space first: the space tap-hold is pending (no deadline started yet); when
//     'n' lands the space tap resolves and the full chord forms -> "no ".
//
// This asserts the INTENDED invariant. It was RED until the bug was fixed by
// freezing the zippychord chord deadline while the layout is still deferring a
// tap-hold decision (see `zchd_tick` in zippychord.rs); both orders now expand to
// "no ". (For reference, the pre-fix buggy event streams were:
//   n-first:     "dn:N t:20ms dn:Space t:6ms up:N t:1ms up:Space"  -> net "n "
//   space-first: "...dn:Space ...dn:BSpace up:BSpace up:N dn:N dn:O ... dn:Space up:Space..." -> net "no ")
#[test]
fn sim_zippy_taphold_chord_press_order_dependent() {
    static CFG: &str = "(defsrc spc n)\
        (deflayer base (tap-hold 200 200 spc (layer-while-held l2)) n)\
        (deflayer l2 spc n)\
        (defzippy file on-first-press-chord-deadline 20 \
         idle-reactivate-time 100 smart-space full)";
    static CONTENT: &str = "\n n\tno\n";

    let n_first = overlap_net_text(
        &simulate_with_zippy_file_content(CFG, "d:n t:5 d:spc t:10 u:n t:5 u:spc t:300", CONTENT)
            .to_ascii(),
    );
    let space_first = overlap_net_text(
        &simulate_with_zippy_file_content(CFG, "d:spc t:5 d:n t:10 u:spc t:5 u:n t:300", CONTENT)
            .to_ascii(),
    );

    assert_eq!(
        n_first, space_first,
        "press order changed the chord output (a chord is a set): n-first={n_first:?} space-first={space_first:?}"
    );
    assert_eq!(
        "no ", n_first,
        "both press orders must expand the \" n\"->\"no\" chord with a smart trailing space"
    );
}

#[test]
fn sim_zippy_leading_space_nondeterministic() {
    // (A) zippy ENABLED: the " a" chord activates; the participating space is
    //     typed eagerly then backspaced away. Net output: "a".
    let enabled = simulate_with_zippy_file_content(
        ZIPPY_CFG_DEADLINE50,
        "d:spc d:a t:300",
        ZIPPY_LEADING_SPACE_CONTENT,
    )
    .to_ascii();
    assert_eq!(
        "dn:Space t:1ms dn:A t:49ms dn:BSpace up:BSpace dn:BSpace up:BSpace up:A dn:A", enabled,
        "enabled -> chord fires -> output \"a\" (space swallowed)"
    );

    // (B) zippy temporarily DISABLED (a non-chord key 'x' was just typed), then
    //     'a' pressed before SPACE -> literal passthrough -> "a ".
    let disabled_a_first = simulate_with_zippy_file_content(
        ZIPPY_CFG_DEADLINE50,
        "d:x u:x t:5 d:a d:spc t:300",
        ZIPPY_LEADING_SPACE_CONTENT,
    )
    .to_ascii();
    assert_eq!(
        "dn:X t:1ms up:X t:4ms dn:A t:1ms dn:Space", disabled_a_first,
        "disabled + a-first -> literal \"a \" (trailing space)"
    );

    // (C) Same as (B) but SPACE pressed before 'a' -> literal " a".
    let disabled_spc_first = simulate_with_zippy_file_content(
        ZIPPY_CFG_DEADLINE50,
        "d:x u:x t:5 d:spc d:a t:300",
        ZIPPY_LEADING_SPACE_CONTENT,
    )
    .to_ascii();
    assert_eq!(
        "dn:X t:1ms up:X t:4ms dn:Space t:1ms dn:A", disabled_spc_first,
        "disabled + space-first -> literal \" a\" (leading space)"
    );
}

// Regression for the real-hardware bug where a leading-space chord activated
// SPACE-FIRST drops its smart-space trailing space (user sees `no` instead of
// `no `). The participating space is typed eagerly as `Space↓` and that key is
// never released before smart-space presses `Space` again, so the output stream
// has two `Space↓` with no `Space↑` between them. A real OS coalesces the two
// into one held key and — since the first one's char was backspaced — the
// trailing space is lost. The net-text oracle can't see this (it counts each
// press as a char), so we assert the *key-state* invariant on the raw stream.
#[test]
fn sim_zippy_leading_space_first_no_double_space_press() {
    static CFG: &str = "(defsrc spc n)(deflayer base spc n)(defzippy file smart-space full)";
    static CONTENT: &str = "\n n\tno\n";
    // space pressed first, then n, both held (no release needed to reproduce).
    let raw = simulate_with_zippy_file_content(CFG, "d:spc d:n t:50", CONTENT).to_spaces();
    println!("space-first raw: {raw}");
    assert_eq!(
        Ok(()),
        super::kanata_proptest::check_no_double_press(&raw),
        "space-first leading-space chord must not press Space twice without a release\n  raw: {raw}"
    );
}

// --- Overlap under-delete regression -------------------------------------
//
// Deterministic regression shrunk from the zippychord property tests
// (`kanata_proptest.rs`). Kept here as a fast, dependency-free repro.

static OVERLAP_UNDERDELETE_CFG: &str =
    "(defsrc lalt)(deflayer base (caps-word 2000))(defzippy file on-first-press-chord-deadline 50)";

/// Reconstruct net visible text from a `to_ascii()` stream: replay key-downs,
/// apply backspaces, honor held shift.
fn overlap_net_text(ascii: &str) -> String {
    let mut out: Vec<char> = Vec::new();
    let mut shift = false;
    for tok in ascii.split_ascii_whitespace() {
        if let Some(key) = tok.strip_prefix("dn:") {
            match key {
                "LShift" | "RShift" => shift = true,
                "BSpace" => {
                    out.pop();
                }
                "Space" => out.push(' '),
                k => {
                    if let Some(c) = overlap_key_to_char(k) {
                        out.push(if shift { c.to_ascii_uppercase() } else { c });
                    }
                }
            }
        } else if let Some(key) = tok.strip_prefix("up:") {
            if matches!(key, "LShift" | "RShift") {
                shift = false;
            }
        }
    }
    out.into_iter().collect()
}

fn overlap_key_to_char(key: &str) -> Option<char> {
    if let Some(d) = key.strip_prefix("Kb") {
        return d.chars().next();
    }
    let mut chars = key.chars();
    match (chars.next(), chars.next()) {
        (Some(c), None) if c.is_ascii_alphabetic() => Some(c.to_ascii_lowercase()),
        _ => None,
    }
}

/// Minimal deterministic reproduction shrunk from the property tests.
///
/// Dictionary: `b`->"c" (exact), ` b`->"cfbcc", ` bd`->"fee ". Pressing b, SPACE,
/// d activates the full chord ` bd` whose output is "fee ". Because `b`->"c"
/// eagerly activates first and "c" is a common prefix of "cfbcc", the backspace
/// accounting for the final ` bd` activation under-counts by one and fails to
/// erase the leading "c", so the result is "cfee " instead of "fee ".
///
/// This is a regression test for the enabled-path backspace under-count: the
/// common-prefix optimization left the kept prefix characters out of the
/// next-activation delete count. Fixed in `zippychord.rs` by seeding that count
/// with the kept-prefix length.
#[test]
fn repro_overlap_underdelete() {
    let content = "\nb\tc\n b\tcfbcc\n bd\tfee \n";
    let ascii = simulate_with_zippy_file_content(
        OVERLAP_UNDERDELETE_CFG,
        "t:600 d:b d:spc d:d t:20 u:spc u:b u:d t:300",
        content,
    )
    .to_ascii();
    assert_eq!(
        "fee ",
        overlap_net_text(&ascii),
        "stray leading char not erased"
    );
}

// --- suppress-space: press-order trailing space (former bug as feature) ------
//
// `suppress-space not-first-def-key`: the trailing smart space is added only
// when the FIRST key the user pressed is the chord definition's first key;
// pressing any other chord key first suppresses it. This turns the historically
// press-order-dependent leading-space behavior into an intentional, opt-in
// control. See ZIPPY_PBT_NOTES / the zippy memory notes for the original bug.

static SUPPRESS_NOT_FIRST_CFG: &str =
    "(defsrc)(deflayer base)(defzippy file smart-space full suppress-space not-first-def-key)";

#[test]
fn sim_zippychord_suppress_space_not_first_def_key() {
    // chord "dy" -> "day"; the definition's first key is 'd'.
    // Press 'd' first (== first def key) -> trailing space kept.
    let d_first = overlap_net_text(
        &simulate_with_zippy_file_content(
            SUPPRESS_NOT_FIRST_CFG,
            "d:d d:y t:10 u:d u:y t:300",
            ZIPPY_FILE_CONTENT,
        )
        .to_ascii(),
    );
    assert_eq!(
        "day ", d_first,
        "d-first matches def -> keep trailing space"
    );

    // Press 'y' first (!= first def key) -> trailing space suppressed.
    let y_first = overlap_net_text(
        &simulate_with_zippy_file_content(
            SUPPRESS_NOT_FIRST_CFG,
            "d:y d:d t:10 u:y u:d t:300",
            ZIPPY_FILE_CONTENT,
        )
        .to_ascii(),
    );
    assert_eq!(
        "day", y_first,
        "y-first differs from def -> suppress trailing space"
    );
}

#[test]
fn sim_zippychord_suppress_space_not_first_def_key_leading_space() {
    // Leading-space chord " n" -> "no"; the definition's first key is SPACE.
    // This is the exact shape of the original press-order bug, now a feature.
    static CFG: &str =
        "(defsrc)(deflayer base)(defzippy file smart-space full suppress-space not-first-def-key)";
    static CONTENT: &str = "\n n\tno\n";

    // SPACE first (== first def key) -> keep trailing space -> "no ".
    let space_first = overlap_net_text(
        &simulate_with_zippy_file_content(CFG, "d:spc d:n t:10 u:spc u:n t:300", CONTENT)
            .to_ascii(),
    );
    assert_eq!(
        "no ", space_first,
        "space-first matches def -> keep trailing space"
    );

    // 'n' first (!= first def key) -> suppress trailing space -> "no".
    let n_first = overlap_net_text(
        &simulate_with_zippy_file_content(CFG, "d:n d:spc t:10 u:n u:spc t:300", CONTENT)
            .to_ascii(),
    );
    assert_eq!(
        "no", n_first,
        "n-first differs from def -> suppress trailing space"
    );
}

// --- suppress-space: dedicated suppress key ---------------------------------
//
// `suppress-space (key bspc)`: holding the configured key while chording
// suppresses the trailing smart space. The key is tracked like a modifier and
// is otherwise passed through, so it must be a key whose passthrough is
// acceptable while chording.

static SUPPRESS_KEY_CFG: &str =
    "(defsrc)(deflayer base)(defzippy file smart-space full suppress-space (key rctl))";

#[test]
fn sim_zippychord_suppress_space_key() {
    // Without the suppress key: trailing space added as usual.
    let normal = overlap_net_text(
        &simulate_with_zippy_file_content(
            SUPPRESS_KEY_CFG,
            "d:d d:y t:10 u:d u:y t:300",
            ZIPPY_FILE_CONTENT,
        )
        .to_ascii(),
    );
    assert_eq!("day ", normal, "no suppress key -> keep trailing space");

    // Suppress key (rctl) held across the chord: trailing space suppressed.
    let suppressed = overlap_net_text(
        &simulate_with_zippy_file_content(
            SUPPRESS_KEY_CFG,
            "d:rctl t:5 d:d d:y t:10 u:d u:y t:5 u:rctl t:300",
            ZIPPY_FILE_CONTENT,
        )
        .to_ascii(),
    );
    assert_eq!(
        "day", suppressed,
        "suppress key held -> suppress trailing space"
    );
}

#[test]
fn sim_zippychord_suppress_space_noop_without_smart_space() {
    // With smart-space disabled there is no trailing space to begin with, so
    // suppress-space is inert regardless of press order.
    static CFG: &str = "(defsrc)(deflayer base)(defzippy file suppress-space not-first-def-key)";
    let d_first = overlap_net_text(
        &simulate_with_zippy_file_content(CFG, "d:d d:y t:10 u:d u:y t:300", ZIPPY_FILE_CONTENT)
            .to_ascii(),
    );
    let y_first = overlap_net_text(
        &simulate_with_zippy_file_content(CFG, "d:y d:d t:10 u:y u:d t:300", ZIPPY_FILE_CONTENT)
            .to_ascii(),
    );
    assert_eq!("day", d_first, "smart-space disabled -> no trailing space");
    assert_eq!("day", y_first, "smart-space disabled -> no trailing space");
}

#[test]
fn sim_zippychord_multikey_followup() {
    // Regression: a followup component with more than one key (`xy ab`) must
    // activate. Previously the first key's partial (subset) match against the
    // pending followup was discarded in favor of a `Neither` main-chord lookup,
    // soft-resetting the followup so it never completed. The generous
    // followup-chord-deadline keeps the followup live across the settle so this test
    // isolates the activation behaviour (the deadline itself is covered separately).
    let cfg = "(defsrc lalt)(deflayer base lalt)(defzippy file on-first-press-chord-deadline 50 followup-chord-deadline 500 idle-reactivate-time 500 smart-space none)";
    let content = "
xy	foo
xy ab	BAR
";
    let result = simulate_with_zippy_file_content(
        cfg,
        "d:x d:y t:1 u:x u:y t:300 d:a d:b t:1 u:a u:b t:300",
        content,
    )
    .to_ascii();
    assert_eq!(
        "dn:X t:1ms dn:BSpace up:BSpace dn:F up:F dn:O up:O dn:O up:O \
         t:1ms up:X t:1ms up:Y t:298ms \
         dn:A t:1ms \
         dn:BSpace up:BSpace dn:BSpace up:BSpace dn:BSpace up:BSpace dn:BSpace up:BSpace \
         dn:LShift up:B dn:B up:LShift dn:LShift up:A dn:A up:LShift dn:LShift dn:R up:R up:LShift \
         t:1ms up:A t:1ms up:B",
        result
    );
}

// Input that exercises a *slow* multi-key followup: root `xy` pressed fast (within
// the tight 50ms initial deadline), released, then followup `ab` pressed with a
// 100ms gap between its two keys — far longer than the 50ms initial deadline.
const SLOW_FOLLOWUP_INPUT: &str = "d:x d:y t:1 u:x u:y t:300 d:a t:100 d:b t:1 u:a u:b t:300";
const SLOW_FOLLOWUP_TSV: &str = "
xy	foo
xy ab	BAR
";

#[test]
fn sim_zippychord_followup_deadline_allows_slow_followup() {
    // With a separate, longer `followup-chord-deadline`, the followup completes
    // even though the gap between its keys exceeds the tight initial deadline.
    let cfg = "(defsrc lalt)(deflayer base lalt)(defzippy file \
               on-first-press-chord-deadline 50 followup-chord-deadline 500 smart-space none)";
    let result =
        simulate_with_zippy_file_content(cfg, SLOW_FOLLOWUP_INPUT, SLOW_FOLLOWUP_TSV).to_ascii();
    assert_eq!("BAR", overlap_net_text(&result));
}

#[test]
fn sim_zippychord_followup_deadline_defaults_to_chord_deadline() {
    // Without `followup-chord-deadline` it falls back to the 50ms initial deadline,
    // so the followup window (which starts when the root keys are released) expires
    // during the gap before the followup completes: zippy disables and the keys pass
    // through (net "fooab", no expansion). Pins the fallback AND shows the option is
    // non-vacuous.
    let cfg = "(defsrc lalt)(deflayer base lalt)(defzippy file \
               on-first-press-chord-deadline 50 smart-space none)";
    let result =
        simulate_with_zippy_file_content(cfg, SLOW_FOLLOWUP_INPUT, SLOW_FOLLOWUP_TSV).to_ascii();
    assert_eq!("fooab", overlap_net_text(&result));
}

// A single-key followup `xy z`->"BAR" after a root `xy`->"foo", to exercise the
// *idle gap* between the word and the followup (the user-reported case: typing
// `do` then `s` seconds later still produced `does`).
const IDLE_FOLLOWUP_TSV: &str = "
xy	foo
xy z	BAR
";

#[test]
fn sim_zippychord_followup_fires_within_idle_deadline() {
    // Followup pressed 200ms after the root — within the 500ms followup deadline —
    // so it still activates.
    let cfg = "(defsrc lalt)(deflayer base lalt)(defzippy file \
               on-first-press-chord-deadline 50 followup-chord-deadline 500 smart-space none)";
    let input = "d:x d:y t:1 u:x u:y t:200 d:z t:1 u:z t:1";
    let result = simulate_with_zippy_file_content(cfg, input, IDLE_FOLLOWUP_TSV).to_ascii();
    assert_eq!("BAR", overlap_net_text(&result));
}

#[test]
fn sim_zippychord_followup_expires_past_idle_deadline() {
    // Followup pressed 600ms after the root — past the 500ms followup deadline — so
    // the pending followup is cancelled and the key passes through (net "fooz", not
    // "BAR"). This is the fix for the followup persisting indefinitely across an idle.
    let cfg = "(defsrc lalt)(deflayer base lalt)(defzippy file \
               on-first-press-chord-deadline 50 followup-chord-deadline 500 smart-space none)";
    let input = "d:x d:y t:1 u:x u:y t:600 d:z t:1 u:z t:1";
    let result = simulate_with_zippy_file_content(cfg, input, IDLE_FOLLOWUP_TSV).to_ascii();
    assert_eq!("fooz", overlap_net_text(&result));
}

#[test]
fn sim_zippychord_redundant_echo_delete() {
    // OPEN efficiency bug discovered by the event-stream observable (#1); see
    // ZIPPY_PBT_NOTES.md. A chord whose output extends its own echoed input keys
    // should PRESERVE that prefix and only append the new suffix — emitting NO
    // backspace. `ab`->"abc": after echoing the typed key(s), completing the chord
    // should just append, not backspace the echoed "a" and retype "abc". Today
    // zippychord backspaces the echoed prefix and retypes it: the net text is the
    // correct "abc", but the redundant delete corrupts the result when it is dropped
    // under load / on a laggy remote (the user-observed failure). The common-prefix
    // optimization in zippychord.rs already preserves prefixes *between expansions*;
    // the fix is to extend it to the echoed input keys. Asserts the intended
    // (efficient) behaviour and currently FAILS.
    let tsv = "ab\tabc\n";
    let cfg = "(defsrc lalt)(deflayer base lalt)(defzippy file \
               on-first-press-chord-deadline 200 smart-space none)";
    let result =
        simulate_with_zippy_file_content(cfg, "d:a t:5 d:b t:5 u:a u:b t:50", tsv).to_ascii();
    assert!(
        !result.contains("BSpace"),
        "zippychord redundantly backspaced the echoed prefix instead of preserving \
         it; output: {result}"
    );
}

// A chord that is a proper prefix of a longer chord defers its expansion: it does not
// fire eagerly (avoiding the over-eager churn `sim_zippychord_no_overeager_expansion`
// guards), but it MUST still fire once the user settles on it — either by pausing past
// the chord deadline, or by releasing the keys. Net text "error" both ways.
#[test]
fn sim_zippychord_deferred_prefix_chord_fires() {
    let content = "er\terror\nres\tresponse\nsure\tsure\n";
    // Settle by pausing: the deferred `er` fires "error" when the deadline elapses.
    let on_pause =
        simulate_with_zippy_file_content(ZIPPY_CFG, "d:e d:r t:300 u:e u:r t:50", content)
            .to_ascii();
    assert_eq!(
        "dn:E t:1ms dn:R t:299ms up:R dn:R dn:O up:O up:R dn:R up:E t:1ms up:R", on_pause,
        "deferred prefix chord did not fire 'error' on the deadline"
    );
    // Settle by releasing: the deferred `er` fires "error" on release, no pause.
    let on_release =
        simulate_with_zippy_file_content(ZIPPY_CFG, "d:e d:r u:e u:r t:50", content).to_ascii();
    assert_eq!(
        "dn:E t:1ms dn:R t:1ms up:R dn:R dn:O up:O up:R dn:R up:E t:1ms up:R", on_release,
        "deferred prefix chord did not fire 'error' on release"
    );
}

/// Chords distilled from a real user config: two words that have followups
/// (`cn`->"can", `do`->"do") followed by a second chord typed in quick succession.
static SUCCESSION_TSV: &str = "\
cn\tcan
cn t\tcant
cn n\tcant
ou\tyou
you\tyou
do\tdo
do g\tdoing
do s\tdoes
do n\tdont
do t\tdont
do d\tdid
do s n\tdoesnt
do d n\tdidnt
it\tit
it s\tits
";

static SUCCESSION_CFG: &str = "(defsrc lalt)(deflayer base lalt)(defzippy file \
    on-first-press-chord-deadline 50 followup-chord-deadline 300 smart-space full)";

// `cn`->"can" then `ou`->"you": the `ou` expansion is lost ("can ou") when the
// followup deadline of `cn` elapses while `ou` is held and deferred.
#[test]
fn sim_zippychord_deferred_chord_dropped_by_followup_deadline() {
    let input = "d:c t:8 d:n t:8 u:c t:2 u:n t:280 d:o t:10 d:u t:30 u:o t:5 u:u t:400";
    let result = simulate_with_zippy_file_content(SUCCESSION_CFG, input, SUCCESSION_TSV).to_ascii();
    assert_eq!("can you ", overlap_net_text(&result), "raw: {result}");
}

// Same sequence outside the 260..=290ms gap window expands correctly.
#[test]
fn sim_zippychord_deferred_chord_survives_short_gap() {
    let input = "d:c t:8 d:n t:8 u:c t:2 u:n t:60 d:o t:10 d:u t:30 u:o t:5 u:u t:400";
    let result = simulate_with_zippy_file_content(SUCCESSION_CFG, input, SUCCESSION_TSV).to_ascii();
    assert_eq!("can you ", overlap_net_text(&result), "raw: {result}");
}

// The followup deadline elapsing between the two presses of `ou` strands the already
// echoed `o`, so the expansion is appended to it ("can oyou ").
#[test]
#[ignore = "known bug: zchd_clear_history strands the echo of a still-held key"]
fn sim_zippychord_echo_stranded_by_followup_deadline() {
    let input = "d:c t:8 d:n t:8 u:c t:2 u:n t:290 d:o t:10 d:u t:30 u:o t:5 u:u t:400";
    let result = simulate_with_zippy_file_content(SUCCESSION_CFG, input, SUCCESSION_TSV).to_ascii();
    assert_eq!("can you ", overlap_net_text(&result), "raw: {result}");
}

// `do`->"do" then `it`->"it" rolled so `t` lands first. A lone `t` after `do` is the
// defined way to type "dont", so the followup fires and `it` can no longer form — the
// keys alone cannot say which word was meant. What this pins is that the previous word
// survives: "dont" stays on screen and the orphaned `i` is appended, rather than the
// whole word being erased and replaced by "it ".
#[test]
fn sim_zippychord_followup_key_pressed_first_keeps_prior_word() {
    let input = "d:d t:8 d:o t:8 u:d t:2 u:o t:60 d:t t:12 d:i t:12 u:t t:5 u:i t:400";
    let result = simulate_with_zippy_file_content(SUCCESSION_CFG, input, SUCCESSION_TSV).to_ascii();
    assert_eq!("dont i", overlap_net_text(&result), "raw: {result}");
}

// A chord typed after a followup appends instead of erasing it.
#[test]
fn sim_zippychord_chord_after_followup_appends() {
    let input =
        "d:d t:8 d:o t:8 u:d t:2 u:o t:60 d:t t:12 u:t t:60 d:c t:8 d:n t:8 u:c t:2 u:n t:400";
    let result = simulate_with_zippy_file_content(SUCCESSION_CFG, input, SUCCESSION_TSV).to_ascii();
    assert_eq!("dont can ", overlap_net_text(&result), "raw: {result}");
}

// The same two chords in typed order are unaffected.
#[test]
fn sim_zippychord_succession_in_order() {
    let input = "d:d t:8 d:o t:8 u:d t:2 u:o t:60 d:i t:12 d:t t:12 u:i t:5 u:t t:400";
    let result = simulate_with_zippy_file_content(SUCCESSION_CFG, input, SUCCESSION_TSV).to_ascii();
    assert_eq!("do it ", overlap_net_text(&result), "raw: {result}");
}

// Gaps across the window where a pending followup used to swallow the deferred `ou`
// expansion, and its edges. 290ms lands between the two presses of `ou` instead of
// after them, which strands the echoed key
// (`sim_zippychord_echo_stranded_by_followup_deadline`).
#[test]
fn sim_zippychord_deferred_chord_fires_across_followup_deadline_window() {
    for gap in [250, 260, 270, 280, 300, 310] {
        let input =
            format!("d:c t:8 d:n t:8 u:c t:2 u:n t:{gap} d:o t:10 d:u t:30 u:o t:5 u:u t:400");
        let result =
            simulate_with_zippy_file_content(SUCCESSION_CFG, &input, SUCCESSION_TSV).to_ascii();
        assert_eq!(
            "can you ",
            overlap_net_text(&result),
            "gap {gap}ms, raw: {result}"
        );
    }
}

// A followup chain reached by releasing each link: `do` -> `do s` -> `do s n`.
#[test]
fn sim_zippychord_followup_chain_released_between_links() {
    let input = "d:d t:8 d:o t:8 u:d t:2 u:o t:60 d:s t:12 u:s t:60 d:n t:12 u:n t:400";
    let result = simulate_with_zippy_file_content(SUCCESSION_CFG, input, SUCCESSION_TSV).to_ascii();
    assert_eq!("doesnt ", overlap_net_text(&result), "raw: {result}");
}

// The same chain with the `s` link still held when `n` arrives — the state the
// input-key clearing changes.
#[test]
fn sim_zippychord_followup_chain_link_still_held() {
    let input = "d:d t:8 d:o t:8 u:d t:2 u:o t:60 d:s t:12 d:n t:12 u:s t:5 u:n t:400";
    let result = simulate_with_zippy_file_content(SUCCESSION_CFG, input, SUCCESSION_TSV).to_ascii();
    assert_eq!("doesnt ", overlap_net_text(&result), "raw: {result}");
}

// The other chain from the same root: `do` -> `do d` -> `do d n`.
#[test]
fn sim_zippychord_followup_chain_did_didnt() {
    let input = "d:d t:8 d:o t:8 u:d t:2 u:o t:60 d:d t:12 u:d t:60 d:n t:12 u:n t:400";
    let result = simulate_with_zippy_file_content(SUCCESSION_CFG, input, SUCCESSION_TSV).to_ascii();
    assert_eq!("didnt ", overlap_net_text(&result), "raw: {result}");
}
