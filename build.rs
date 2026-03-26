fn main() -> Result<(), Box<dyn std::error::Error>> {
    tonic_build::configure()
        .build_server(true) // needed for proxy mode (local gRPC server)
        .build_client(true) // needed to connect to Azure Functions Host
        .compile_protos(
            &[
                "proto/FunctionRpc.proto",
                "proto/shared/NullableTypes.proto",
                "proto/identity/ClaimsIdentityRpc.proto",
            ],
            &["proto"],
        )?;
    Ok(())
}
