//! Prints the API's OpenAPI document, which `just api-spec` writes to `api/openapi.json`.

fn main() {
  let document = reseek::api::openapi()
    .to_pretty_json()
    .expect("the OpenAPI document serializes");
  println!("{document}");
}
