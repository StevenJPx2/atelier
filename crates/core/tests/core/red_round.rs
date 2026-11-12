//! A test that fails on purpose, for the end-to-end check of #20.

#[test]
fn a_red_round() {
    assert_eq!(1, 2, "red round 5");
}
