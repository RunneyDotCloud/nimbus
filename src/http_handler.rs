use lambda_http::{tracing, Body, Error, Request, RequestExt, RequestPayloadExt, Response};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
struct RequestBody {
    name: String,
    code: String,
}

pub(crate) async fn function_handler(event: Request) -> Result<Response<Body>, Error> {
    let body = event.body();
    let s = std::str::from_utf8(body).expect("invalid utf-8");
    tracing::info!(payload = %s, "JSON Payload received");

    let request_body: RequestBody = serde_json::from_slice(body.as_ref())?;

    let data = match serde_json::from_str::<RequestBody>(s) {
        Ok(data) => data,
        Err(err) => {
            let resp = Response::builder()
                .status(400)
                .header("content-type", "text/html")
                .body(err.to_string().into())
                .map_err(Box::new)?;
            return Ok(resp);
        }
    };

    // Return something that implements IntoResponse.
    // It will be serialized to the right response event automatically by the runtime
    let resp = Response::builder()
        .status(200)
        .header("content-type", "text/html")
        .body(message.into())
        .map_err(Box::new)?;
    Ok(resp)
}

#[cfg(test)]
mod tests {
    use super::*;
    use lambda_http::{Request, RequestExt};
    use std::collections::HashMap;

    #[tokio::test]
    async fn test_generic_http_handler() {
        let request = Request::default();

        let response = function_handler(request).await.unwrap();
        assert_eq!(response.status(), 200);

        let body_bytes = response.body().to_vec();
        let body_string = String::from_utf8(body_bytes).unwrap();

        assert_eq!(
            body_string,
            "Hello world, this is an AWS Lambda HTTP request"
        );
    }

    #[tokio::test]
    async fn test_http_handler_with_query_string() {
        let mut query_string_parameters: HashMap<String, String> = HashMap::new();
        query_string_parameters.insert("name".into(), "nimbus".into());

        let request = Request::default().with_query_string_parameters(query_string_parameters);

        let response = function_handler(request).await.unwrap();
        assert_eq!(response.status(), 200);

        let body_bytes = response.body().to_vec();
        let body_string = String::from_utf8(body_bytes).unwrap();

        assert_eq!(
            body_string,
            "Hello nimbus, this is an AWS Lambda HTTP request"
        );
    }
}
