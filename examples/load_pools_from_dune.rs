    use duners::{client::DuneClient, error::DuneRequestError};
    use rust::constants::{Env, DUNE_QUERY_ID};
    use serde::Deserialize;

    #[derive(Deserialize, Debug, PartialEq)]
    struct ResultStruct {
        max_block: u64
    }

    #[tokio::main]
    async fn main() -> Result<(), DuneRequestError> {
        dotenv::dotenv().ok();

        let env = Env::new();
        /* 24279386 */
        let dune = DuneClient::new(env.dune_api_key.as_str());
        let results = dune.refresh::<ResultStruct>(DUNE_QUERY_ID, None, None).await?;
        println!("{:?}", results.get_rows());
        Ok(())
    }