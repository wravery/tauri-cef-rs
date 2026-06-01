use quote::{format_ident, quote};
use std::{
    fmt::{self, Display, Formatter},
    fs,
    io::Write,
    iter::Iterator,
    path::{Path, PathBuf},
    process::Command,
};

pub fn generate_bindings(source_path: &Path) -> crate::Result<PathBuf> {
    let bindings = crate::read_bindings(source_path)?;
    let parsed = syn::parse_file(&bindings)?;
    let parse_tree = ParseTree::from(&parsed);

    let mut out_file = crate::dirs::get_out_dir();
    out_file.push("bindings.rs");
    let mut bindings = fs::File::create(&out_file)?;
    write!(bindings, "{parse_tree}")?;
    format_bindings(&out_file)?;

    Ok(out_file)
}

enum Resource {
    Control(String),
    Packed(String),
    String(String),
}

#[derive(Default)]
struct ParseTree {
    resources: Vec<Resource>,
}

impl ParseTree {
    pub fn write_prelude(&self, f: &mut Formatter<'_>) -> fmt::Result {
        let header = quote! {
            use cef_dll_sys::{
                cef_id_for_command_id_name, cef_id_for_pack_resource_name, cef_id_for_pack_string_name,
            };
            use std::sync::OnceLock;
        }
        .to_string();
        writeln!(f, "{header}")
    }

    pub fn write_resources(&self, f: &mut Formatter<'_>) -> fmt::Result {
        for resource in &self.resources {
            let name = match resource {
                Resource::Control(name) | Resource::Packed(name) | Resource::String(name) => {
                    name.as_str()
                }
            };
            let Ok(name_lit) = syn::parse_str::<syn::LitCStr>(&format!(r#"c"{name}""#)) else {
                continue;
            };
            let cef_fn = format_ident!(
                "{}",
                match resource {
                    Resource::Control(_) => "cef_id_for_command_id_name",
                    Resource::Packed(_) => "cef_id_for_pack_resource_name",
                    Resource::String(_) => "cef_id_for_pack_string_name",
                }
            );
            let name_ident = format_ident!("{}", name.to_lowercase());
            writeln!(
                f,
                "\n/// Load the resource ID for [`{name}`] with [`{cef_fn}`]."
            )?;
            let getter = quote! {
                pub fn #name_ident() -> Option<i32> {
                    static RESOURCE_ID: OnceLock<Option<i32>> = OnceLock::new();
                    *RESOURCE_ID.get_or_init(|| {
                        let resource_id = unsafe { #cef_fn(#name_lit.as_ptr()) };
                        if resource_id == -1 {
                            None
                        } else {
                            Some(resource_id)
                        }
                    })
                }
            }
            .to_string();
            writeln!(f, "{getter}")?;
        }
        Ok(())
    }

    pub fn write_tests(&self, f: &mut Formatter<'_>) -> fmt::Result {
        let asserts = self.resources.iter().map(|resource| {
            let name = match resource {
                Resource::Control(name) | Resource::Packed(name) | Resource::String(name) => name,
            };
            let cef_fn = format_ident!("{}", name.to_lowercase());
            let sys_id = format_ident!("{name}");
            quote! {
                if let Some(resource_id) = #cef_fn() {
                    assert_eq!(resource_id, sys::#sys_id);
                }
            }
        });
        let tests = quote! {
            #[cfg(test)]
            mod test {
                use super::*;
                use crate::*;

                #[test]
                fn test_id_mapping() {
                    test_init_cef();

                    #(#asserts)*
                }
            }
        }
        .to_string();
        write!(f, "\n{tests}")
    }
}

impl Display for ParseTree {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        self.write_prelude(f)?;
        self.write_resources(f)?;
        self.write_tests(f)
    }
}

impl From<&syn::File> for ParseTree {
    fn from(value: &syn::File) -> Self {
        let mut tree = Self::default();

        tree.resources = value
            .items
            .iter()
            .filter_map(|item| match item {
                syn::Item::Const(syn::ItemConst {
                    vis: syn::Visibility::Public(_),
                    ident,
                    ..
                }) => {
                    let name = ident.to_string();
                    if name.starts_with("IDC_") {
                        Some(Resource::Control(name))
                    } else if name.starts_with("IDR_") {
                        Some(Resource::Packed(name))
                    } else if name.starts_with("IDS_") {
                        Some(Resource::String(name))
                    } else {
                        None
                    }
                }
                _ => None,
            })
            .collect();

        tree
    }
}

fn format_bindings(source_path: &Path) -> crate::Result<()> {
    let mut cmd = Command::new("cargo");
    cmd.args(&["fmt", "--", &source_path.display().to_string()]);
    cmd.output()?;
    Ok(())
}
