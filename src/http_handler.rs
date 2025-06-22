use fs_extra::dir::{copy, CopyOptions};
use lambda_http::{tracing, Body, Error, Request, RequestExt, RequestPayloadExt, Response};
use serde::{Deserialize, Serialize};
use std::{env, path::Path};
use tokio::{fs::create_dir_all, process::Command, try_join};

#[derive(Debug, Serialize, Deserialize)]
struct RequestBody {
    component_id: String,
    code: String,
}

fn error_response(status: u16, message: String) -> Result<Response<Body>, Error> {
    let resp = Response::builder()
        .status(status)
        .header("content-type", "text/html")
        .body(message.into())
        .map_err(Box::new)?;
    Ok(resp)
}

fn success_response(message: String) -> Result<Response<Body>, Error> {
    let resp = Response::builder()
        .status(200)
        .header("content-type", "text/html")
        .body(message.into())
        .map_err(Box::new)?;
    Ok(resp)
}

pub(crate) async fn function_handler(event: Request) -> Result<Response<Body>, Error> {
    // ENVIRONMENT VARIABLES
    let bucket_name = env::var("S3_BUCKET_NAME").map_err(|_| "S3_BUCKET_NAME not set")?;
    let cloudfront_domain =
        env::var("CLOUDFRONT_DOMAIN").map_err(|_| "CLOUDFRONT_DOMAIN not set")?;
    let region = env::var("AWS_REGION").map_err(|_| "AWS_REGION not set")?;
    let lambda_task_root = env::var("LAMBDA_TASK_ROOT").map_err(|_| "LAMBDA_TASK_ROOT not set")?;

    let body = event.body();
    let s = std::str::from_utf8(body).expect("invalid utf-8");

    tracing::info!(payload = %s, "JSON Payload received");

    let request_body: RequestBody = serde_json::from_slice(body.as_ref())?;

    let data = match serde_json::from_str::<RequestBody>(s) {
        Ok(data) => data,
        Err(err) => {
            return error_response(400, err.to_string());
        }
    };

    let component_id = &data.component_id;

    let workspace_dir = Path::new("/tmp").join(component_id);
    let src_dir = workspace_dir.join("src");
    let out_dir = workspace_dir.join("dist");

    tracing::info!(
        component_id = component_id,
        "Creating isolated workspace at {}",
        workspace_dir.display()
    );

    let templates_path = Path::new(&lambda_task_root).join("templates");

    let output = Command::new("cp")
        .arg("-r")
        .arg(&templates_path)
        .arg(&workspace_dir)
        .output()
        .await
        .map_err(|e| format!("Failed to copy templates: {}", e))?;

    if !output.status.success() {
        let error_msg = String::from_utf8_lossy(&output.stderr);
        tracing::error!(error = %error_msg, "Failed to copy templates");
        return error_response(500, format!("Failed to copy templates: {}", error_msg));
    }

    tracing::info!("Successfully copied templates to workspace");

    match try_join!(create_dir_all(&src_dir), create_dir_all(&out_dir)) {
        Ok(_) => {
            tracing::info!(
                "Successfully created workspace directories: src={}, out={}",
                src_dir.display(),
                out_dir.display()
            );
        }
        Err(e) => {
            tracing::error!(error = %e, "Failed to create workspace directories");
            return error_response(
                500,
                format!("Failed to create workspace directories: {}", e),
            );
        }
    }

    tracing::info!(component_id = component_id, "Writing component and CSS ");

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
