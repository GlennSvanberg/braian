//! Loopback HTTP against the same store calls, for one heavy object.

use std::time::{Duration, Instant};

use anyhow::Context;
use openpim_model::{AttributePatch, Object, ObjectPatch, ValuePatch};
use openpim_rules::CompiledModel;
use openpim_store::admin::{self, PgConn};
use openpim_store::{ObjectOp, STORAGE_FORM, Store};
use openpim_synth::{EditKind, Tier, definitions, edits, objects_iter};
use serde_json::{Map, Value};
use tokio::io::{AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream};

const DATABASE: &str = "openpim_latency";
const TOKENS: &str = "ada:human:latency-ada";
const TOKEN: &str = "latency-ada";

pub async fn run(database_url: &str, samples: usize, seed: u64) -> anyhow::Result<()> {
    if samples < 2 {
        anyhow::bail!("samples must be at least 2");
    }
    let mut admin_config = admin::parse_config(database_url)?;
    admin_config.dbname("postgres");
    let admin = PgConn::connect(admin_config).await?;
    admin::drop_database(&admin.client, DATABASE).await?;
    admin::create_database(&admin.client, DATABASE).await?;
    drop(admin);

    let mut config = admin::parse_config(database_url)?;
    config.dbname(DATABASE);
    let store = Store::connect_config(config, STORAGE_FORM).await?;
    let defs = definitions(Tier::Heavy, seed);
    let identities = openpim_api::identities(&store, TOKENS).await?;
    let actor = identities[0].actor.clone();
    store.put_definitions(&actor, &defs).await?;
    let model = CompiledModel::compile(&defs)
        .map_err(|errors| anyhow::anyhow!("heavy definitions: {errors:?}"))?;

    let app = openpim_api::router(store.clone(), identities);
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let port = listener.local_addr()?.port();
    let server = tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    let mut http = HttpClient::connect(port).await?;

    let warmup = objects_iter(&defs, Tier::Heavy, seed, 0..3);
    for object in warmup {
        let body = create_body(&object);
        let (status, response) = http.call("POST", "/v1/objects", Some(&body)).await?;
        if status != 200 {
            anyhow::bail!(
                "warmup create returned {status}: {}",
                String::from_utf8_lossy(&response)
            );
        }
    }

    let mut http_create = Vec::with_capacity(samples);
    for object in objects_iter(&defs, Tier::Heavy, seed, 1_000..1_000 + samples) {
        let body = create_body(&object);
        let started = Instant::now();
        let (status, response) = http.call("POST", "/v1/objects", Some(&body)).await?;
        let elapsed = started.elapsed();
        if status != 200 {
            anyhow::bail!(
                "create returned {status}: {}",
                String::from_utf8_lossy(&response)
            );
        }
        http_create.push(elapsed);
    }

    let mut store_create = Vec::with_capacity(samples);
    for object in objects_iter(&defs, Tier::Heavy, seed, 2_000..2_000 + samples) {
        let started = Instant::now();
        store
            .write_batch(&actor, vec![ObjectOp::Create(object)])
            .await?;
        store_create.push(started.elapsed());
    }

    let mut patch_objects = Vec::with_capacity(samples * 2);
    for object in objects_iter(&defs, Tier::Heavy, seed, 3_000..3_000 + samples * 2) {
        store
            .write_batch(&actor, vec![ObjectOp::Create(object.clone())])
            .await?;
        patch_objects.push(object);
    }

    let mut http_patch = Vec::with_capacity(samples);
    let mut store_patch = Vec::with_capacity(samples);
    for (index, object) in patch_objects.iter().enumerate() {
        let edit = edits(
            &defs,
            Tier::Heavy,
            seed,
            EditKind::Mixed,
            (3_000 + index)..(3_001 + index),
        )
        .next()
        .context("edit")?;
        let patch = first_three(&edit.patch);
        if index < samples {
            let body = patch_body(&patch);
            let path = format!("/v1/objects/{}", object.id.as_uuid());
            let started = Instant::now();
            let (status, response) = http
                .call_with("PATCH", &path, &[("if-match", "1")], Some(&body))
                .await?;
            let elapsed = started.elapsed();
            if status != 200 {
                anyhow::bail!(
                    "patch returned {status}: {}",
                    String::from_utf8_lossy(&response)
                );
            }
            http_patch.push(elapsed);
        } else {
            let started = Instant::now();
            store
                .write_batch(
                    &actor,
                    vec![ObjectOp::Update {
                        id: object.id,
                        patch: ObjectPatch {
                            values: patch,
                            groups: Default::default(),
                        },
                        base_version: 1,
                        read_version: 1,
                    }],
                )
                .await?;
            store_patch.push(started.elapsed());
        }
    }

    let sample = objects_iter(&defs, Tier::Heavy, seed, 1..2)
        .next()
        .context("object")?;
    let mut validate = Vec::with_capacity(samples);
    for _ in 0..samples {
        let started = Instant::now();
        let errors = model.validate(&sample);
        validate.push(started.elapsed());
        if !errors.is_empty() {
            anyhow::bail!("heavy object did not validate");
        }
    }

    server.abort();
    println!("api-latency heavy seed {seed} samples {samples}");
    print_row("create http", &http_create);
    print_row("create store", &store_create);
    print_row("patch http", &http_patch);
    print_row("patch store", &store_patch);
    print_row("validate", &validate);
    Ok(())
}

fn first_three(patch: &ValuePatch) -> ValuePatch {
    ValuePatch {
        attributes: patch
            .attributes
            .iter()
            .take(3)
            .map(|(id, value)| (*id, value.clone()))
            .collect(),
    }
}

fn create_body(object: &Object) -> Vec<u8> {
    let mut values = Map::new();
    for (id, value) in &object.values {
        values.insert(id.as_uuid().to_string(), value.clone());
    }
    let mut groups = Map::new();
    for (structure, group) in &object.groups {
        groups.insert(
            structure.as_uuid().to_string(),
            Value::String(group.as_uuid().to_string()),
        );
    }
    let body = serde_json::json!({
        "id": object.id.as_uuid(),
        "type": object.type_id.as_uuid(),
        "groups": groups,
        "values": values,
    });
    serde_json::to_vec(&body).expect("create body")
}

fn patch_body(patch: &ValuePatch) -> Vec<u8> {
    let mut values = Map::new();
    for (id, attribute) in &patch.attributes {
        let wire = match attribute {
            AttributePatch::Replace { value } => value.clone(),
            AttributePatch::Remove => Value::Null,
            AttributePatch::Languages { values } => {
                let mut languages = Map::new();
                for (language, value) in values {
                    languages.insert(language.to_string(), value.clone().unwrap_or(Value::Null));
                }
                Value::Object(languages)
            }
        };
        values.insert(id.as_uuid().to_string(), wire);
    }
    serde_json::to_vec(&serde_json::json!({ "values": values })).expect("patch body")
}

fn print_row(name: &str, samples: &[Duration]) {
    let mut millis: Vec<f64> = samples
        .iter()
        .map(|sample| sample.as_secs_f64() * 1_000.0)
        .collect();
    millis.sort_by(f64::total_cmp);
    println!(
        "{name:<14} p50 {:>8.2} ms   p95 {:>8.2} ms   p99 {:>8.2} ms",
        percentile(&millis, 0.50),
        percentile(&millis, 0.95),
        percentile(&millis, 0.99),
    );
}

fn percentile(sorted: &[f64], p: f64) -> f64 {
    let index = (p * (sorted.len() - 1) as f64).ceil() as usize;
    sorted[index.min(sorted.len() - 1)]
}

struct HttpClient {
    stream: BufReader<TcpStream>,
}

impl HttpClient {
    async fn connect(port: u16) -> anyhow::Result<Self> {
        let stream = TcpStream::connect(("127.0.0.1", port)).await?;
        Ok(Self {
            stream: BufReader::new(stream),
        })
    }

    async fn call(
        &mut self,
        method: &str,
        path: &str,
        body: Option<&[u8]>,
    ) -> anyhow::Result<(u16, Vec<u8>)> {
        self.call_with(method, path, &[], body).await
    }

    async fn call_with(
        &mut self,
        method: &str,
        path: &str,
        extra: &[(&str, &str)],
        body: Option<&[u8]>,
    ) -> anyhow::Result<(u16, Vec<u8>)> {
        let mut head = format!(
            "{method} {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nAuthorization: Bearer {TOKEN}\r\nConnection: keep-alive\r\n"
        );
        for (name, value) in extra {
            head.push_str(name);
            head.push_str(": ");
            head.push_str(value);
            head.push_str("\r\n");
        }
        if let Some(body) = body {
            head.push_str("Content-Type: application/json\r\n");
            head.push_str(&format!("Content-Length: {}\r\n", body.len()));
        }
        head.push_str("\r\n");
        self.stream.write_all(head.as_bytes()).await?;
        if let Some(body) = body {
            self.stream.write_all(body).await?;
        }
        self.stream.flush().await?;

        let mut headers = Vec::new();
        loop {
            let byte = self.stream.read_u8().await?;
            headers.push(byte);
            if headers.ends_with(b"\r\n\r\n") {
                break;
            }
            if headers.len() > 65_536 {
                anyhow::bail!("response headers exceeded 64KB");
            }
        }
        let text = String::from_utf8_lossy(&headers);
        let status = text
            .split_whitespace()
            .nth(1)
            .context("status line")?
            .parse::<u16>()?;
        let length = text
            .lines()
            .find_map(|line| {
                let (name, value) = line.split_once(':')?;
                name.eq_ignore_ascii_case("content-length")
                    .then(|| value.trim().parse::<usize>().ok())
                    .flatten()
            })
            .unwrap_or(0);
        let mut response = vec![0; length];
        if length > 0 {
            self.stream.read_exact(&mut response).await?;
        }
        Ok((status, response))
    }
}
