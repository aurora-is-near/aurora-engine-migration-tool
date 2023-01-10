use crate::{
    indexer::Indexer,
    rpc::{BlockKind, Client},
};

#[tokio::test]
async fn test_client() {
    let worker = workspaces::sandbox().await.unwrap();
    let port = worker.rpc_port();
    let mut client = Client::new_with_url(&format!("http://localhost:{port}"));
    // worker.fast_forward(1000).await.unwrap();
    let _account = worker.dev_create_account().await.unwrap();
    let block = client.get_block(BlockKind::Latest).await.unwrap();

    assert_eq!(block.0, 2);
}

#[ignore]
#[tokio::test]
async fn test_migration() {
    let dir = tempfile::tempdir().unwrap();
    let worker = workspaces::sandbox().await.unwrap();
    let port = worker.rpc_port();
    let input = dir.path().join("input");
    let output = dir.path().join("output");
    let url = format!("http://localhost:{port}");

    worker.fast_forward(2).await.unwrap();
    let mut indexer = Indexer::new_with_url(&input, true, Some(2), &url).unwrap();
    indexer.run_n_blocks(5).await.unwrap();

    let result = crate::migration::Migration::prepare_indexed_with_url(input, output, &url).await;

    result.unwrap();
    // assert!(result.is_ok());
}
