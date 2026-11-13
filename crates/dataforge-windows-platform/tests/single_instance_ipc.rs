use dataforge_windows_platform::{
    ForwardPayload, NamedPipeServer, SingleInstanceLock, is_forwardable_target,
    send_to_existing_instance,
};
use uuid::Uuid;

type TestResult = Result<(), Box<dyn std::error::Error>>;

#[test]
fn single_instance_lock_blocks_second_acquire_until_dropped() -> TestResult {
    let mutex_name = format!(r"Local\DataForge-Test-{}", Uuid::new_v4());

    let primary = SingleInstanceLock::try_acquire(&mutex_name)?;
    assert!(primary.is_some());

    let secondary = SingleInstanceLock::try_acquire(&mutex_name)?;
    assert!(secondary.is_none());

    drop(primary);

    let reacquired = SingleInstanceLock::try_acquire(&mutex_name)?;
    assert!(reacquired.is_some());
    Ok(())
}

#[test]
fn named_pipe_forwards_database_files_and_connection_urls() -> TestResult {
    let pipe_name = format!(r"\\.\pipe\DataForge-Test-{}", Uuid::new_v4());
    let mut server = NamedPipeServer::bind(&pipe_name)?;

    let expected = ForwardPayload {
        targets: vec![
            r"C:\data\analytics.duckdb".to_owned(),
            r"C:\data\local.sqlite".to_owned(),
            "postgres://alice@localhost:5432/appdb".to_owned(),
        ],
    };

    let client_pipe = pipe_name.clone();
    let client_payload = expected.clone();
    let sender =
        std::thread::spawn(move || send_to_existing_instance(&client_pipe, &client_payload));

    let received = server.accept_one()?;
    let send_result = sender
        .join()
        .map_err(|_| std::io::Error::other("sender thread panicked"))?;
    send_result?;

    assert_eq!(received, expected);
    Ok(())
}

#[test]
fn is_forwardable_target_recognizes_db_files_and_urls() {
    assert!(is_forwardable_target(r"C:\tmp\demo.sqlite"));
    assert!(is_forwardable_target(r"C:\tmp\demo.SQLITE3"));
    assert!(is_forwardable_target(r"C:\tmp\warehouse.duckdb"));
    assert!(is_forwardable_target("postgres://localhost/db"));
    assert!(is_forwardable_target("mysql://user@127.0.0.1:3306/test"));
    assert!(is_forwardable_target("dataforge://open?conn=1"));

    assert!(!is_forwardable_target("--help"));
    assert!(!is_forwardable_target(r"C:\tmp\notes.txt"));
}
