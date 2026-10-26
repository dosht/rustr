#![allow(dead_code)]
use std::{collections::VecDeque, sync::{Arc,Mutex}, time::Duration};
use axum::{Router, body::{Body,to_bytes}, extract::{Request,State}, http::{HeaderMap,StatusCode}, response::Response};
use serde_json::Value;

#[derive(Clone,Debug)]
pub struct Recorded { pub method:String, pub path:String, pub query:String, pub headers:HeaderMap, pub body:Vec<u8> }
#[derive(Clone)]
pub struct Reply { pub status:u16, pub body:Vec<u8>, pub headers:Vec<(String,String)>, pub delay:Duration }
impl Reply {
    pub fn json(status:u16,value:Value) -> Self {
        Self {status,body:serde_json::to_vec(&value).unwrap(),headers:vec![("content-type".into(),"application/json".into())],delay:Duration::ZERO}
    }
    pub fn text(status:u16,value:&str) -> Self {
        Self {status,body:value.as_bytes().to_vec(),headers:vec![],delay:Duration::ZERO}
    }
    pub fn header(mut self,name:&str,value:&str) -> Self {self.headers.push((name.into(),value.into()));self}
    pub fn delayed(mut self,delay:Duration) -> Self {self.delay=delay;self}
}
#[derive(Clone,Default)]
struct Fixture { replies:Arc<Mutex<VecDeque<Reply>>>, requests:Arc<Mutex<Vec<Recorded>>> }
pub struct Server { pub url:String, fixture:Fixture, task:tokio::task::JoinHandle<()> }
impl Drop for Server {fn drop(&mut self){self.task.abort();}}
impl Server {
    pub async fn start() -> Self {
        let fixture=Fixture::default();
        let app=Router::new().fallback(handle).with_state(fixture.clone());
        let listener=tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url=format!("http://{}",listener.local_addr().unwrap());
        let task=tokio::spawn(async move{axum::serve(listener,app).await.unwrap()});
        Self{url,fixture,task}
    }
    pub fn reply(&self,reply:Reply){self.fixture.replies.lock().unwrap().push_back(reply);}
    pub fn requests(&self)->Vec<Recorded>{self.fixture.requests.lock().unwrap().clone()}
    pub fn pending(&self)->usize{self.fixture.replies.lock().unwrap().len()}
}
async fn handle(State(fixture):State<Fixture>,request:Request)->Response {
    let (parts,body)=request.into_parts();
    let body=to_bytes(body,128*1024*1024).await.unwrap().to_vec();
    fixture.requests.lock().unwrap().push(Recorded {method:parts.method.to_string(),path:parts.uri.path().into(),query:parts.uri.query().unwrap_or_default().into(),headers:parts.headers,body});
    let reply={fixture.replies.lock().unwrap().pop_front()};
    match reply {
        Some(reply)=>{
            tokio::time::sleep(reply.delay).await;
            let mut response=Response::builder().status(StatusCode::from_u16(reply.status).unwrap());
            for (name,value) in reply.headers {response=response.header(name,value);}
            response.body(Body::from(reply.body)).unwrap()
        },
        None=>Response::builder().status(599).body(Body::from("unscripted request")).unwrap(),
    }
}
