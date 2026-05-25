# lava-serverless

Typed `(deflava-serverless-function …)` abstracting AWS Lambda, GCP
Functions, and Cloudflare Workers behind one shape.

```lisp
(deflava-serverless-function api-handler
  :provider aws
  :runtime nodejs20
  :handler "index.handler"
  :code-uri "./dist/handler.zip"
  :memory 512
  :timeout 30
  :env (:NODE_ENV "production" :LOG_LEVEL "info"))
```

## Surface

- `ServerlessFunction` — provider-agnostic typed shape
- `Provider = Aws | Gcp | CloudflareWorker`
- `functions_in_source(src) -> Vec<ServerlessFunction>`
- `render_terraform_resources(&f) -> Vec<(type_id, name, body)>`

The renderer emits:

| Provider | Terraform resource |
|---|---|
| Aws | `aws_lambda_function` |
| Gcp | `google_cloudfunctions_function` |
| CloudflareWorker | `cloudflare_workers_script` |

7/7 unit tests cover form extraction, missing-clause + unknown-provider
errors, per-provider rendering, serde round-trip.
