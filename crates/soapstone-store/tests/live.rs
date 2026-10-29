use soapstone_core::{fetch_all, to_url};
use soapstone_store::Store;

const OPTITRAVEL: &str = "https://www2.optitravel.net/optitravel/export/wbs/opti_res/wbs_server.php";

#[tokio::test]
#[ignore = "network: the user's real service"]
async fn optitravel_import_send_and_store_the_response() {
    let docs = tokio::task::spawn_blocking(|| fetch_all(&to_url(OPTITRAVEL).unwrap())).await.unwrap().unwrap();
    let mut s = Store::open_in_memory().unwrap();
    s.import_service(1, docs, &Default::default()).unwrap();
    let tree = s.tree(1).unwrap();
    assert_eq!(tree[0].operations.len(), 18);
    let id = tree[0].operations.iter().find(|o| o.name == "AVITIK_GETGROUPRESERVES").unwrap().requests[0].id;
    let (call, vars) = s.prepare_send(id).unwrap();
    let mut resp = soapstone_http::send(&soapstone_http::client(), &call, &vars).await.unwrap();
    assert!(resp.status > 0 && !resp.body.is_empty(), "{resp:?}");
    s.save_response(id, &resp).unwrap();
    resp.pretty = None;
    assert_eq!(s.request(id).unwrap().last_response, Some(resp));
}
