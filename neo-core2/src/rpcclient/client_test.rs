#[cfg(test)]
mod tests {
    use super::*;
    use url::Url;
    use anyhow::Result;

    #[test]
    fn test_get_endpoint() -> Result<()> {
        let host = "http://localhost:1234";
        let u = Url::parse(host)?;
        let client = Client {
            endpoint: u,
        };
        assert_eq!(host, client.endpoint().as_str());
        Ok(())
    }
}
