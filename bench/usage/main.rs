use std::fmt::Write;

use cornucopia::conn::cornucopia_conn;
use criterion::{BenchmarkId, Criterion};
use diesel::{Connection, PgConnection};
use postgres::{fallible_iterator::FallibleIterator, Client, NoTls};
use tokio::runtime::Runtime;

const QUERY_SIZE: &[usize] = &[1, 100, 10_000];
const INSERT_SIZE: &[usize] = &[1, 100, 1000];

mod cornucopia_benches;
mod diesel_benches;
mod postgres_benches;
mod tokio_postgres_benches;

fn clear(client: &mut Client) {
    client
    .batch_execute("TRUNCATE TABLE comments CASCADE;TRUNCATE TABLE posts CASCADE;TRUNCATE TABLE users CASCADE").unwrap();
}

fn prepare_client(
    size: usize,
    client: &mut Client,
    hair_color_init: impl Fn(usize) -> Option<&'static str>,
) {
    clear(client);
    let mut query = String::from("INSERT INTO users (name, hair_color) VALUES");
    let mut params = Vec::with_capacity(2 * size);

    for x in 0..size {
        write!(
            &mut query,
            "{} (${}, ${})",
            if x == 0 { "" } else { "," },
            2 * x + 1,
            2 * x + 2
        )
        .unwrap();
        params.push((format!("User {}", x), hair_color_init(x)));
    }

    let params = params
        .iter()
        .flat_map(|(a, b)| [a as _, b as _])
        .collect::<Vec<_>>();

    client.execute(&query, &params).unwrap();
}

fn prepare_full(client: &mut Client) {
    prepare_client(100, client, |i| {
        Some(if i % 2 == 0 { "black" } else { "brown" })
    });

    let user_ids = client
        .query_raw("SELECT id FROM users", std::iter::empty::<u32>())
        .unwrap()
        .map(|row| Ok(row.get("id")))
        .collect::<Vec<i32>>()
        .unwrap();

    let data = user_ids
        .iter()
        .flat_map(|user_id| {
            (0..10).map(move |i| (format!("Post {} by user {}", i, user_id), user_id, None))
        })
        .collect::<Vec<_>>();

    let mut insert_query = String::from("INSERT INTO posts(title, user_id, body) VALUES");

    for x in 0..data.len() {
        write!(
            insert_query,
            "{} (${}, ${}, ${})",
            if x == 0 { "" } else { "," },
            3 * x + 1,
            3 * x + 2,
            3 * x + 3
        )
        .unwrap();
    }

    let data = data
        .iter()
        .flat_map(|(title, user_id, body): &(_, _, Option<String>)| {
            [title as _, user_id as _, body as _]
        })
        .collect::<Vec<_>>();

    client.execute(&insert_query as &str, &data).unwrap();

    let all_posts = client
        .query_raw("SELECT id FROM posts", std::iter::empty::<u32>())
        .unwrap()
        .map(|row| Ok(row.get("id")))
        .collect::<Vec<i32>>()
        .unwrap();

    let data = all_posts
        .iter()
        .flat_map(|post_id| {
            (0..10).map(move |i| (format!("Comment {} on post {}", i, post_id), post_id))
        })
        .collect::<Vec<_>>();

    let mut insert_query = String::from("INSERT INTO comments(text, post_id) VALUES");

    for x in 0..data.len() {
        write!(
            insert_query,
            "{} (${}, ${})",
            if x == 0 { "" } else { "," },
            2 * x + 1,
            2 * x + 2,
        )
        .unwrap();
    }

    let data = data
        .iter()
        .flat_map(|(title, post_id)| [title as _, post_id as _])
        .collect::<Vec<_>>();

    client.execute(&insert_query, &data).unwrap();
}

fn bench(c: &mut Criterion) {
    cornucopia::container::cleanup(false).ok();
    cornucopia::container::setup(false).unwrap();
    let client = &mut cornucopia_conn().unwrap();
    let rt: &'static Runtime = Box::leak(Box::new(Runtime::new().unwrap()));
    let async_client = &mut rt.block_on(async {
        let (client, conn) = tokio_postgres::connect(
            "postgresql://postgres:postgres@127.0.0.1:5435/postgres",
            NoTls,
        )
        .await
        .unwrap();
        rt.spawn(conn);
        client
    });
    let conn =
        &mut PgConnection::establish("postgresql://postgres:postgres@127.0.0.1:5435/postgres")
            .unwrap();
    cornucopia::load_schema(client, &["usage/cornucopia_benches/schema.sql"]).unwrap();
    {
        let mut group = c.benchmark_group("bench_trivial_query");
        for size in QUERY_SIZE {
            prepare_client(*size, client, |_| None);
            group.bench_function(BenchmarkId::new("diesel", size), |b| {
                diesel_benches::bench_trivial_query(b, conn)
            });
            group.bench_function(BenchmarkId::new("postgres", size), |b| {
                postgres_benches::bench_trivial_query(b, client);
            });
            group.bench_function(BenchmarkId::new("tokio_postgres", size), |b| {
                tokio_postgres_benches::bench_trivial_query(b, async_client);
            });
            group.bench_function(BenchmarkId::new("cornucopia", size), |b| {
                cornucopia_benches::sync::bench_trivial_query(b, client);
            });
            group.bench_function(BenchmarkId::new("cornucopia_async", size), |b| {
                cornucopia_benches::bench_trivial_query(b, async_client);
            });
        }
        group.finish();
    }
    {
        let mut group = c.benchmark_group("bench_medium_complex_query");
        for size in QUERY_SIZE {
            prepare_client(*size, client, |i| {
                Some(if i % 2 == 0 { "black" } else { "brown" })
            });
            group.bench_function(BenchmarkId::new("diesel", size), |b| {
                diesel_benches::bench_medium_complex_query(b, conn)
            });
            group.bench_function(BenchmarkId::new("postgres", size), |b| {
                postgres_benches::bench_medium_complex_query(b, client);
            });
            group.bench_function(BenchmarkId::new("tokio_postgres", size), |b| {
                tokio_postgres_benches::bench_medium_complex_query(b, async_client);
            });
            group.bench_function(BenchmarkId::new("cornucopia", size), |b| {
                cornucopia_benches::sync::bench_medium_complex_query(b, client);
            });
            group.bench_function(BenchmarkId::new("cornucopia_async", size), |b| {
                cornucopia_benches::bench_medium_complex_query(b, async_client);
            });
        }
        group.finish();
    }
    {
        let mut group = c.benchmark_group("bench_loading_associations_sequentially");
        prepare_full(client);
        group.bench_function("diesel", |b| {
            diesel_benches::loading_associations_sequentially(b, conn)
        });
        group.bench_function("postgres", |b| {
            postgres_benches::loading_associations_sequentially(b, client)
        });
        group.bench_function("tokio_postgres", |b| {
            tokio_postgres_benches::loading_associations_sequentially(b, async_client);
        });
        group.bench_function("cornucopia", |b| {
            cornucopia_benches::sync::loading_associations_sequentially(b, client)
        });
        group.bench_function("cornucopia_async", |b| {
            cornucopia_benches::loading_associations_sequentially(b, async_client)
        });
        group.finish();
    }
    {
        let mut group = c.benchmark_group("bench_insert");
        for size in INSERT_SIZE {
            group.bench_with_input(BenchmarkId::new("diesel", size), size, |b, i| {
                clear(client);
                diesel_benches::bench_insert(b, conn, *i)
            });
            group.bench_with_input(BenchmarkId::new("postgres", size), size, |b, i| {
                clear(client);
                postgres_benches::bench_insert(b, client, *i);
            });
            group.bench_with_input(BenchmarkId::new("tokio_postgres", size), size, |b, i| {
                tokio_postgres_benches::bench_insert(b, async_client, *i);
            });
            group.bench_with_input(BenchmarkId::new("cornucopia", size), size, |b, i| {
                clear(client);
                cornucopia_benches::sync::bench_insert(b, client, *i);
            });
            group.bench_with_input(BenchmarkId::new("cornucopia_async", size), size, |b, i| {
                clear(client);
                cornucopia_benches::bench_insert(b, async_client, *i);
            });
        }
        group.finish();
    }

    cornucopia::container::cleanup(false).unwrap();
}
criterion::criterion_group!(benches, bench);
criterion::criterion_main!(benches);
