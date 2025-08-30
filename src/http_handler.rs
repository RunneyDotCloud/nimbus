use aws_sdk_s3::{primitives::ByteStream, Client};
use fs_extra::dir::{copy, CopyOptions};
use lambda_http::{tracing, Body, Error, Request, RequestExt, Response};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::{env, path::Path};
use tokio::{
    fs::{self, create_dir_all, write},
    process::Command,
    try_join,
};

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
        .arg("/tmp/")
        .output()
        .await
        .map_err(|e| format!("Failed to execute cp command: {}", e))?;

    if !output.status.success() {
        let error_msg = String::from_utf8_lossy(&output.stderr);
        return error_response(500, format!("cp command failed: {}", error_msg));
    }

    let temp_templates_dir = Path::new("/tmp/templates");
    if let Err(e) = fs::rename(&temp_templates_dir, &workspace_dir).await {
        return error_response(500, format!("Failed to rename templates directory: {}", e));
    }

    tracing::info!(
        component_id = component_id,
        "Successfully copied templates to workspace"
    );

    tracing::info!(component_id = component_id, "Writing component and CSS ");

    create_dir_all(&src_dir)
        .await
        .map_err(|e| format!("Failed to create src directory: {}", e))?;
    create_dir_all(&out_dir)
        .await
        .map_err(|e| format!("Failed to create out directory: {}", e))?;

    let globals_source = workspace_dir.join("globals.css");
    let globals_dest = src_dir.join("globals.css");

    if let Err(e) = fs::copy(&globals_source, &globals_dest).await {
        tracing::error!(
            error = %e,
            source = %globals_source.display(),
            dest = %globals_dest.display(),
            "Failed to copy globals.css"
        );
        return error_response(500, format!("Failed to copy globals.css: {}", e));
    }

    tracing::info!(
        source = %globals_source.display(),
        dest = %globals_dest.display(),
        "Successfully copied globals.css"
    );

    if let Err(e) = write(src_dir.join("UserComponent.tsx"), &data.code).await {
        tracing::error!(error = %e, "Failed to write component file");
        return error_response(500, format!("Failed to write component file: {}", e));
    }

    let entry_point = format!(
        r#"
    import React from 'react';
    import ReactDOM from 'react-dom/client';
    import UserComponent from './UserComponent';
    import './globals.css';
    
    const rootEl = document.getElementById('root');
    if (rootEl) ReactDOM.createRoot(rootEl).render(<UserComponent />);
    "#
    );

    if let Err(e) = write(src_dir.join("index.tsx"), &entry_point).await {
        tracing::error!(error = %e, "Failed to write entry point");
        return error_response(500, format!("Failed to write component file: {}", e));
    }

    tracing::info!(component_id = component_id, "Successfully copied TSXs");

    tracing::info!(component_id = component_id, "Starting Bun bundling");

    let bun_output = Command::new("/usr/local/bin/bun")
        .arg("build")
        .arg("./src/index.tsx")
        .arg("--outdir")
        .arg("./dist")
        .arg("--target")
        .arg("browser")
        .current_dir(&workspace_dir)
        .output()
        .await
        .map_err(|e| format!("Failed to execute bun build: {}", e))?;

    if !bun_output.status.success() {
        let stderr = String::from_utf8_lossy(&bun_output.stderr);
        return error_response(500, format!("Bun build failed: {}", stderr));
    }

    tracing::info!(component_id = component_id, "Starting tailwind build");

    let tailwind_input_path = src_dir.join("globals.css");
    let tailwind_output_path = out_dir.join("index.css");
    let tailwind_command = Command::new("/usr/local/bin/bun")
        .arg("x")
        .arg("tailwindcss")
        .arg("-i")
        .arg(&tailwind_input_path)
        .arg("-o")
        .arg(&tailwind_output_path)
        .current_dir(&workspace_dir)
        .output()
        .await
        .map_err(|e| format!("Failed to execute tailwind build: {}", e))?;

    if !tailwind_command.status.success() {
        let stderr = String::from_utf8_lossy(&tailwind_command.stderr);
        return error_response(500, format!("Tailwind build failed: {}", stderr));
    }

    tracing::info!(component_id = component_id, "Generating HTML");

    let html_content = format!(
        r#"<!DOCTYPE html>
      <html lang="en">
        <head>
          <meta charset="UTF-8" />
          <meta name="viewport" content="width=device-width, initial-scale=1.0" />
          <title>Rendered Component</title>
          <link rel="stylesheet" href="./index.css" />
        </head>
        <body>
          <div id="root"></div>
          <script type="module" src="./index.js"></script>
        </body>
      </html>"#
    );

    write(out_dir.join("index.html"), html_content).await?;

    let s3_config = aws_config::load_defaults(aws_config::BehaviorVersion::latest()).await;
    let s3_client = Client::new(&s3_config);

    let mut dir_entries = fs::read_dir(&out_dir).await?;

    while let Some(entry) = dir_entries.next_entry().await? {
        let file_path = entry.path();
        let file_name = file_path.file_name().and_then(|n| n.to_str()).unwrap();

        let s3_key = format!("{}/{}", component_id, file_name);

        if let Err(e) = upload_file_to_s3(&s3_client, &bucket_name, &file_path, &s3_key).await {
            return error_response(500, format!("Upload failed: {}", e));
        }
    }

    let response_body = json!({
        "renderUrl": format!("https://{}.preview.runney.cloud/index.html", component_id),
        "originalUrl": format!("https://{}/{}/index.html", cloudfront_domain, component_id)
    });

    if let Err(e) = tokio::fs::remove_dir_all(&workspace_dir).await {
        tracing::error!(
            component_id = component_id,
            error = %e,
            "Failed to cleanup workspace"
        );
    }

    Response::builder()
        .status(200)
        .header("content-type", "application/json")
        .body(response_body.to_string().into())
        .map_err(Box::new)
        .map_err(Into::into)
}

async fn upload_file_to_s3(
    client: &Client,
    bucket_name: &str,
    file_path: &Path,
    s3_key: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let file_content = fs::read(file_path).await?;

    let content_type = match file_path.extension().and_then(|ext| ext.to_str()) {
        Some("css") => "text/css",
        Some("js") => "application/javascript",
        Some("html") => "text/html",
        _ => "application/octet-stream",
    };

    client
        .put_object()
        .bucket(bucket_name)
        .key(s3_key)
        .body(ByteStream::from(file_content))
        .content_type(content_type)
        .send()
        .await?;

    Ok(())
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
