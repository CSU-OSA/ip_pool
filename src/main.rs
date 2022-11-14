use std::collections::HashSet;
use std::sync::{Arc, Mutex};
use std::thread::sleep;
use std::time::Duration;

use actix_web::{App, get, HttpServer, Responder};
use actix_web::web::Data;
use http::header::{HeaderMap, HeaderValue, HOST, USER_AGENT};
use regex::*;

#[get("/")]
async fn ip_pool_response(data: Data<Arc<Mutex<Vec<String>>>>) -> impl Responder {
    let ip_list = data.lock().unwrap();
    let response = serde_json::to_string(&*ip_list).unwrap();
    response
}

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    //初始化ip池
    let ip_pool = Arc::new(Mutex::new(get_new_ip().await.unwrap()));
    println!("initial success!");
    //ip_pool自动爬取更新线程
    let pool = ip_pool.clone();
    let t1 = tokio::spawn(async move {
        update(pool).await;
    });
    //启动web服务
    let pool = ip_pool.clone();
    let data = Data::new(pool);
    HttpServer::new(move || {
        App::new()
            .app_data(data.clone())
            .service(ip_pool_response)
    })
        .bind(("127.0.0.1", 10002))?
        .run()
        .await
        .expect("httpserver start error!");
    //启动ip_pool爬取更新线程
    t1.await.unwrap();
    Ok(())
}


async fn update(old_ip: Arc<Mutex<Vec<String>>>) {
    loop {
        //每隔300s更新一次ip池
        sleep(Duration::from_secs(300));
        //获取新的ip池
        let new_ip = get_new_ip().await.unwrap();
        {
            //获得当前ip池的写锁
            let mut tmp_old_ip = Vec::new();
            {
                //获得当前ip池的内容
                //这里需要注意的是，在该代码块进行clone完毕后，old_ip会被drop，从而释放锁
                //不影响其他线程对于old_ip的访问
                let old_ip = old_ip.lock().unwrap();
                tmp_old_ip = old_ip.clone();
            }
            //获得old_ip的中可用ip的集合
            let useful_old_ip = {
                let mut ans: Vec<String> = Vec::new();
                //不为空则进行筛选
                if tmp_old_ip.len() != 0 {
                    let useful_old_ip = Arc::new(Mutex::new(Vec::new()));
                    let mut flag = 0;
                    let mut threads = Vec::new();
                    //分割old_ip，每个线程处理一部分
                    for i in 1..tmp_old_ip.len() {
                        let mut tmp_pool = vec![];
                        //以每次100个ip分割old_ip
                        if tmp_old_ip.len() >= i * 100 {
                            for j in (i - 1) * 100..i * 100 {
                                tmp_pool.push(tmp_old_ip[j].clone());
                            }
                        } else {
                            //最后一次筛选
                            flag = 1;
                            for j in (i - 1) * 100..tmp_old_ip.len() {
                                tmp_pool.push(tmp_old_ip[j].clone());
                            }
                        }
                        //多线程筛选
                        let uoi = useful_old_ip.clone();
                        let t = tokio::spawn(async move {
                            let tmp_ip_pool = ip_is_useful(tmp_pool).await;
                            match tmp_ip_pool {
                                Ok(mut tmp_ip_pool) => {
                                    //获得可用ip的写锁，将可用ip加入到useful_old_ip中
                                    uoi.lock().unwrap().append(&mut tmp_ip_pool);
                                }
                                Err(_) => {
                                    // println!("ip is not useful");
                                    //TODO!
                                }
                            }
                        });
                        //将线程加入到线程池中
                        threads.push(t);
                        //当筛选完最后一次时，跳出循环
                        if flag == 1 {
                            break;
                        }
                    }
                    //等待所有线程结束
                    for t in threads {
                        t.await.unwrap();
                    }
                    //获得可用ip的读锁
                    ans = useful_old_ip.lock().unwrap().clone();
                    ans
                } else {
                    //如果old_ip为空，则直接返回空
                    ans
                }
            };
            //获得new_ip与useful_old_ip的并集
            let mut useful_ip = Vec::new();
            {
                //使用hashset进行去重
                let mut hashset: HashSet<String> = HashSet::new();
                for ip in useful_old_ip.iter() {
                    hashset.insert(ip.clone());
                }
                //将new_ip中的ip加入到hashset中
                //当useful_old_ip内的可用ip数量达到1000时，停止加入
                //防止useful_ip过大，造成内存占用过大
                if useful_old_ip.len() < 100000 {
                    for ip in new_ip.iter() {
                        hashset.insert(ip.clone());
                    }
                }
                //将hashset中的ip加入到useful_ip中
                //得到去重后的useful_ip
                for i in hashset {
                    useful_ip.push(i);
                }
            }
            //获得old_ip的写锁
            //将old_ip更新为useful_ip
            let mut old_ip = old_ip.lock().unwrap();
            old_ip.clear();
            //clear()后，old_ip的长度为0,但是内存并未释放,此时append()将会开辟新的内存
            //若此时append(),其内存大小为memory(old_ip)+memory(useful_ip)
            //因此，使用shrink_to_fit()释放内存
            old_ip.shrink_to_fit();
            old_ip.append(&mut useful_ip);
        }
        println!("update success!");
    }
}

async fn get_new_ip() -> Result<Vec<String>, Box<dyn std::error::Error>> {
    let ip_pool: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(vec![]));
    let pool = ip_pool.clone();
    let t1 = tokio::spawn(async move {
        get_kuai_ip(pool).await;
    });
    let pool = ip_pool.clone();
    let t2 = tokio::spawn(async move {
        get_89_ip(pool).await;
    });
    let pool = ip_pool.clone();
    let t3 = tokio::spawn(async move {
        get_89api_ip(pool).await;
    });
    t1.await.unwrap();
    t2.await.unwrap();
    t3.await.unwrap();
    let result = ip_pool.lock().unwrap().clone();
    Ok(result)
}

async fn get_kuai_ip(tmp_pool: Arc<Mutex<Vec<String>>>) {
    //使用多线程爬取快代理的ip
    let mut threads = vec![];
    //爬取快代理的ip40页
    for i in 1..40 {
        let tp = tmp_pool.clone();
        let t = tokio::spawn(async move {
            let tmp_kuai_pool = get_ip_pool_kuai(format!("https://www.kuaidaili.com/free/inha/{}/", i)).await;
            let tmp_ip_pool = ip_is_useful(tmp_kuai_pool).await;
            match tmp_ip_pool {
                Ok(mut tmp_ip_pool) => {
                    tp.lock()
                        .unwrap()
                        .append(&mut tmp_ip_pool);
                }
                Err(_) => {
                    // println!("ip is not useful");
                    //TODO!
                }
            }
        });
        threads.push(t);
    }
    for t in threads {
        t.await.unwrap();
    }
}

async fn get_89_ip(tmp_pool: Arc<Mutex<Vec<String>>>) {
    let mut threads = vec![];
    for i in 1..40 {
        let tp = tmp_pool.clone();
        let t = tokio::spawn(async move {
            let tmp_89_pool = get_ip_pool_89(format!("http://www.89ip.cn/index_{}.html", i)).await;
            let tmp_ip_pool = ip_is_useful(tmp_89_pool).await;
            match tmp_ip_pool {
                Ok(mut tmp_ip_pool) => {
                    tp.lock()
                        .unwrap()
                        .append(&mut tmp_ip_pool);
                }
                Err(_) => {
                    // println!("ip is not useful");
                    //TODO!
                }
            }
        });
        threads.push(t);
    }
    for t in threads {
        t.await.unwrap();
    }
}

async fn get_89api_ip(ip_pool: Arc<Mutex<Vec<String>>>) {
    //使用多线程爬取89api的ip
    let tmp_89_api_pool = get_ip_pool_89ip_api().await;
    let mut threads = vec![];
    //由于89api一次性返回的ip数量较多，因此需要分批次进行处理
    let mut count = tmp_89_api_pool.len();
    for i in 1..=30 {
        let mut tmp_pool = vec![];
        for j in (i - 1) * 100..i * 100 {
            //当count等于1时，说明已经没有ip了，直接跳出循环
            if count == 1 {
                break;
            }
            count -= 1;
            tmp_pool.push(tmp_89_api_pool[j].clone());
        }
        let tp = ip_pool.clone();
        let t = tokio::spawn(async move {
            let tmp_ip_pool = ip_is_useful(tmp_pool.to_vec()).await;
            match tmp_ip_pool {
                Ok(mut tmp_ip_pool) => {
                    tp.lock()
                        .unwrap()
                        .append(&mut tmp_ip_pool);
                }
                Err(_) => {
                    // println!("ip is not useful");
                    //TODO!
                }
            }
        });
        threads.push(t);
        //当count等于1时，说明已经没有ip了，直接跳出循环
        if count == 1 {
            break;
        }
    }
    for t in threads {
        t.await.unwrap();
    }
}

async fn ip_is_useful(ip_pool: Vec<String>) -> Result<Vec<String>, reqwest::Error> {
    let mut ip_pool_useful = Vec::new();
    for i in ip_pool {
        //若ip为空，则跳过
        if i == "" {
            continue;
        }
        //若ip不为空，则进行测试
        //使用代理访问百度，若返回的状态码为200，则说明该ip可用
        let url = format!("http://baidu.com");
        let proxies = reqwest::Proxy::http(format!("http://{}", i))?;
        let client = reqwest::Client::builder()
            .proxy(proxies)
            .build()?;
        let res = client.get(&url)
            .headers({
                let mut headers = HeaderMap::new();
                headers.insert(USER_AGENT, HeaderValue::from_static("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/89.0.4389.114 Safari/537.36"));
                headers
            })
            .timeout(Duration::from_secs(5))//设置5s超时时间
            .send()
            .await;
        match res {
            Ok(..) => {
                // println!("{} is useful", i);
                ip_pool_useful.push(i);
            }
            Err(_) => {
                // println!("{} is useless", i)
            }
        }
    }
    Ok(ip_pool_useful)
}

async fn get_ip_pool_89ip_api() -> Vec<String> {
    let mut ip_pool = Vec::new();
    let res = reqwest::Client::new();
    let res = res.get("http://api.89ip.cn/tqdl.html?api=1&num=3000&port=&address=&isp=")
        .headers({
            let mut headers = HeaderMap::new();
            headers.insert(USER_AGENT, HeaderValue::from_static("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/89.0.4389.114 Safari/537.36"));
            headers.insert(HOST, HeaderValue::from_static("api.89ip.cn"));
            headers
        })
        .send()
        .await
        .unwrap();
    let body = res.text().await.unwrap();
    // println!("{:?}", body);
    //将获取到的ip进行正则处理
    let re = Regex::new(r"r>(.*?)<").unwrap();
    let m = re.captures_iter(&body);
    for (_, ip_address) in m.enumerate() {
        ip_pool.push(ip_address[1].to_string());
    }
    // for ip in ip_pool.iter() {
    //     println!("{}", ip);
    // }
    ip_pool
}

async fn get_ip_pool_89(url: String) -> Vec<String> {
    let mut ip_pool = Vec::new();
    let res = reqwest::Client::new();
    let res = res.get(url)
        .headers({
            let mut headers = HeaderMap::new();
            headers.insert(USER_AGENT, HeaderValue::from_static("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/89.0.4389.114 Safari/537.36"));
            headers.insert(HOST, HeaderValue::from_static("www.89ip.cn"));
            headers
        })
        .send()
        .await
        .unwrap();
    let body = res.text().await.unwrap();
    // println!("{:?}", body);
    let re1 = Regex::new("\\t(\\d{1,5})\\t").unwrap();
    let re2 = Regex::new("((2((5[0-5])|([0-4]\\d)))|([0-1]?\\d{1,2}))(\\.((2((5[0-5])|([0-4]\\d)))|([0-1]?\\d{1,2}))){3}").unwrap();
    let port = re1.captures_iter(&body);
    // println!("{:?}", m1);
    // for (j, ip_address) in m1.enumerate() {
    //     println!("{:?}", j);
    //     println!("{:?}",ip_address);
    // }
    let ip = re2.captures_iter(&body);
    // println!("{:?}", m2);
    // for (j, ip_address) in m2.enumerate() {
    //     println!("{:?}", j);
    //     println!("{:?}",ip_address);
    // }
    let mut ip_address_port: Vec<(String, String)> = Vec::new();
    for (_, port) in port.enumerate() {
        ip_address_port.push(("".to_string(), port[1].to_string()));
    }
    for (i, ip_address) in ip.enumerate() {
        ip_address_port[i].0 = ip_address[0].to_string();
    }
    for ip_address_port in ip_address_port.iter() {
        ip_pool.push(format!("{}:{}", ip_address_port.0, ip_address_port.1));
    }
    // for ip in ip_pool.iter() {
    //     println!("{}", ip);
    // }
    ip_pool
}

async fn get_ip_pool_kuai(url: String) -> Vec<String> {
    let mut ip_pool = Vec::new();
    let res = reqwest::Client::new();
    let res = res.get(url)
        .headers({
            let mut headers = HeaderMap::new();
            headers.insert(USER_AGENT, HeaderValue::from_static("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/80.0.3987.163 Safari/537.36"));
            // headers.insert(HOST, HeaderValue::from_static("www.kuaidaili.com"));
            headers
        })
        .send()
        .await
        .unwrap();
    let body = res.text_with_charset("utf-8").await.unwrap();
    // println!("{}", &body);
    let re = Regex::new("IP\">(.*?)</td>[\\s\\S]*?PORT\">(.*?)</td>").unwrap();
    let m = re.captures_iter(&body);
    // println!("{:?}", m);
    for (_, ip_address) in m.enumerate() {
        // println!("{:?}", j);
        // println!("{:?}", ip_address);
        ip_pool.push(format!("{}:{}", ip_address[1].to_string(), ip_address[2].to_string()));
    }
    // for ip in ip_pool.iter() {
    //     println!("{}", ip);
    // }
    ip_pool
}

