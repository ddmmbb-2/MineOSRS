// Extracted, shared checks for create/update; deliberately identical boundary rules.
fn validate_server_resources(name: &str, port: u16, xms_mb: u32, xmx_mb: u32) -> ApiResult<()> {
    if name.is_empty() || name.chars().count() > 60 || port == 0 {
        return Err(bad_request("請輸入有效的 Server 名稱與 Port"));
    }
    if xms_mb < 256 || xmx_mb < xms_mb || xmx_mb > 262_144 {
        return Err(bad_request("記憶體必須滿足 256MB <= Xms <= Xmx <= 262144MB"));
    }
    Ok(())
}

fn validate_java_command(java: &str) -> ApiResult<()> {
    if java.is_empty() || java.contains('\0') {
        return Err(bad_request("Java 執行路徑不正確"));
    }
    Ok(())
}

#[cfg(test)]
mod shared_validation_tests {
    use super::*;

    #[test]
    fn resource_boundaries_match_v07() {
        assert!(validate_server_resources("a", 25565, 256, 256).is_ok());
        assert!(validate_server_resources("a", 65535, 256, 262_144).is_ok());
        for (name, port, xms, xmx) in [
            ("",25565,256,512), ("a",0,256,512), ("a",25565,255,512),
            ("a",25565,1024,512), ("a",25565,256,262_145)
        ] {
            assert!(validate_server_resources(name,port,xms,xmx).is_err());
        }
        assert!(validate_server_resources(&"a".repeat(61),25565,256,512).is_err());
    }

    #[test]
    fn java_boundaries_match_v07() {
        assert!(validate_java_command("java").is_ok());
        assert!(validate_java_command("C:\\JDK 21\\bin\\java.exe").is_ok());
        assert!(validate_java_command("").is_err());
        assert!(validate_java_command("java\0.exe").is_err());
    }

    #[test]
    fn ports_stay_consistent_for_bedrock_ipv4_and_ipv6() {
        assert_eq!(bedrock_ipv6_port(19132),19133);
        assert_eq!(bedrock_ipv6_port(65535),65534);
        assert!(managed_port_conflict("bedrock",19132,"bedrock",19133));
        assert!(!managed_port_conflict("bedrock",19132,"bedrock",19134));
        assert!(managed_port_conflict("paper",25565,"bedrock",25565));
        assert!(!managed_port_conflict("paper",25565,"paper",25566));
    }
}
