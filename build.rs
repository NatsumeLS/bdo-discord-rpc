use std::path::PathBuf;

fn main() {
    println!("cargo:rerun-if-changed=assets/icon.ico");
    println!("cargo:rerun-if-changed=Cargo.toml");
    println!("cargo:rerun-if-changed=Cargo.lock");

    let mut resource = winresource::WindowsResource::new();
    resource.set_icon("assets/icon.ico");
    if let Err(e) = resource.compile() {
        println!("cargo:warning=Could not embed the Icon: {e}");
    }

    let libraries = libraries().unwrap_or_else(|e| {
        println!("cargo:warning=Could not list the Libraries: {e}");
        Vec::new()
    });
    let body: String = libraries
        .iter()
        .map(|(name, license)| format!("({name:?}, {license:?}),"))
        .collect();
    let out = PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("libraries.rs");
    std::fs::write(out, format!("&[{body}]")).unwrap();
}

fn libraries() -> Result<Vec<(String, String)>, String> {
    let cargo = std::env::var("CARGO").map_err(|e| e.to_string())?;
    let manifest = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap()).join("Cargo.toml");
    let output = std::process::Command::new(cargo)
        .args([
            "metadata",
            "--format-version",
            "1",
            "--offline",
            "--manifest-path",
        ])
        .arg(manifest)
        .output()
        .map_err(|e| e.to_string())?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).into_owned());
    }
    let metadata: serde_json::Value =
        serde_json::from_slice(&output.stdout).map_err(|e| e.to_string())?;

    let root = metadata["resolve"]["root"]
        .as_str()
        .ok_or("no root package")?;
    let node = metadata["resolve"]["nodes"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|node| node["id"] == root)
        .ok_or("no root node")?;

    let mut libraries: Vec<(String, String)> = node["deps"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|dep| {
            dep["dep_kinds"]
                .as_array()
                .into_iter()
                .flatten()
                .any(|kind| kind["kind"].is_null())
        })
        .filter_map(|dep| {
            metadata["packages"]
                .as_array()?
                .iter()
                .find(|package| package["id"] == dep["pkg"])
        })
        .map(|package| {
            (
                package["name"].as_str().unwrap_or_default().to_string(),
                package["license"].as_str().unwrap_or("Unknown").to_string(),
            )
        })
        .collect();
    libraries.sort();
    Ok(libraries)
}
