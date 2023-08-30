use warp::Filter;

#[tokio::main]
async fn main() {
    // GET /hello/warp => 200 OK with body "Hello, warp!"
    let index = warp::get()
        .and(warp::path::end())
        .map(|| "Welcome to BL Coordinator");
    let hello = warp::path!("hello" / String).map(|name| format!("Hello, {}!", name));
    let router = index.or(hello);
    warp::serve(router).run(([127, 0, 0, 1], 3030)).await;
}
