use aws_sdk_cloudwatchlogs::{error::SdkError, types::QueryStatus};

pub async fn fetch_logs(
    log_group_name: String,
    start: i64,
    end: i64,
) -> Result<Vec<String>, String> {
    let config = aws_config::load_from_env().await;
    let client = aws_sdk_cloudwatchlogs::Client::new(&config);
    let query_id = match client
        .start_query()
        .set_start_time(Some(start))
        .set_end_time(Some(end))
        .set_query_string(Some("fields @message".into()))
        .set_log_group_name(log_group_name.into())
        .send()
        .await
    {
        Ok(response) => response.query_id,
        Err(e) => return Err(e.to_string()),
    };

    loop {
        tokio::time::sleep(std::time::Duration::from_millis(250)).await;

        match client
            .get_query_results()
            .set_query_id(query_id.clone())
            .send()
            .await
        {
            Ok(response) => {
                let messages = response
                    .results
                    .unwrap_or_default()
                    .into_iter()
                    .flatten()
                    .filter(|result| result.field == Some("@message".to_string()))
                    .map(|result| result.value.unwrap_or_default())
                    .rev()
                    .collect::<Vec<String>>();

                match response.status {
                    Some(QueryStatus::Complete) => return Ok(messages),
                    Some(status @ (QueryStatus::Failed | QueryStatus::Timeout)) => {
                        return Err(status.to_string())
                    }
                    _ => {}
                }
            }
            Err(e) => panic!("Error: {:?}", e),
        };
    }
}
