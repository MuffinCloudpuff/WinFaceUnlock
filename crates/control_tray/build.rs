fn main() -> Result<(), Box<dyn std::error::Error>> {
    #[cfg(windows)]
    {
        embed_resource::compile("windows_resources.rc", embed_resource::NONE)
            .manifest_optional()?;
    }
    Ok(())
}
