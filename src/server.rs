use actix_web::App;
use actix::{Actor, StreamHandler};
use actix_web::{web, Error, HttpRequest, HttpResponse, HttpServer};
use actix_web_actors::ws;
use tokio::task;

pub fn server_init() {
    task::spawn(async {
        HttpServer::new(|| App::new().route("/ws/", web::get().to(index)))
            .bind(("0.0.0.0", 8080))
            .expect("绑定地址失败")
            .run()
            .await
            .expect("启动服务器失败");
    });
}

struct MyWs;

impl Actor for MyWs {
    type Context = ws::WebsocketContext<Self>;
}

impl StreamHandler<Result<ws::Message, ws::ProtocolError>> for MyWs {
    fn handle(&mut self, msg: Result<ws::Message, ws::ProtocolError>, ctx: &mut Self::Context) {
        match msg {
            Ok(ws::Message::Ping(msg)) => ctx.pong(&msg),
            Ok(ws::Message::Text(text)) => ctx.text(text),
            Ok(ws::Message::Binary(bin)) => ctx.binary(bin),
            _ => (),
        }
    }
}

async fn index(req: HttpRequest, stream: web::Payload) -> Result<HttpResponse, Error> {
    let resp = ws::start(MyWs {}, &req, stream);
    println!("{:?}", resp);
    resp
}
