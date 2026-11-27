//! `leonida version` reads these at compile time; a cached build must pick up
//! a new release number.

fn main() {
    println!("cargo:rerun-if-env-changed=RELEASE_BUILD_NUMBER");
    println!("cargo:rerun-if-env-changed=RELEASE_BUILD_COMMIT");
}
