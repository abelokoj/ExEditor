use std::{env, fs, path::PathBuf};

fn main() {
    println!("cargo:rerun-if-changed=assets/ExEditor.rc");
    println!("cargo:rerun-if-changed=assets/ExEditor.ico");
    println!("cargo:rerun-if-changed=assets/ExEditor.manifest");
    println!("cargo:rerun-if-changed=Cargo.toml");
    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }

    let version = env::var("CARGO_PKG_VERSION").expect("Cargo provides the package version");
    let components = ["MAJOR", "MINOR", "PATCH"].map(|component| {
        env::var(format!("CARGO_PKG_VERSION_{component}"))
            .expect("Cargo provides each version component")
            .parse::<u16>()
            .expect("Windows version components must fit in 16 bits")
    });
    let [major, minor, patch] = components;
    let output = PathBuf::from(env::var_os("OUT_DIR").expect("Cargo provides OUT_DIR"));
    let manifest_path = output.join("ExEditor.manifest");
    let manifest = fs::read_to_string("assets/ExEditor.manifest")
        .expect("reading the ExEditor manifest template")
        .replace("@ASSEMBLY_VERSION@", &format!("{major}.{minor}.{patch}.0"));
    fs::write(&manifest_path, manifest).expect("writing the versioned ExEditor manifest");

    let icon_path = PathBuf::from(
        env::var_os("CARGO_MANIFEST_DIR").expect("Cargo provides the project directory"),
    )
    .join("assets/ExEditor.ico");
    let resource = fs::read_to_string("assets/ExEditor.rc")
        .expect("reading the ExEditor resource template")
        .replace("@VERSION_NUMBER@", &format!("{major},{minor},{patch},0"))
        .replace("@VERSION_STRING@", &version)
        .replace(
            "@ICON_PATH@",
            &icon_path.to_string_lossy().replace('\\', "/"),
        )
        .replace(
            "@MANIFEST_PATH@",
            &manifest_path.to_string_lossy().replace('\\', "/"),
        );
    let resource_path = output.join("ExEditor.rc");
    fs::write(&resource_path, resource).expect("writing the versioned ExEditor resource");
    embed_resource::compile(&resource_path, embed_resource::NONE)
        .manifest_required()
        .expect("embedding the required ExEditor Windows manifest and version resource");
}
