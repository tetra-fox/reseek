//! The derive refuses attributes it can not use, with the reason at the attribute.

#[test]
fn bad_attributes_do_not_compile() {
  trybuild::TestCases::new().compile_fail("tests/ui/*.rs");
}
