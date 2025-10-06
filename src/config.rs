use std::{
    error::Error,
    path::{self, Path, PathBuf},
    str::FromStr,
};

use yaml_rust2::{Yaml, YamlLoader};

pub(crate) struct Configuration {
    pub(crate) namespace: String,
    pub(crate) apps: Vec<ProgramSpec>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ProgramSpec {
    pub(crate) working_directory: PathBuf,
    pub(crate) command: String,
    pub(crate) name: String,
    pub(crate) deps: Vec<String>,
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub(crate) enum InvalidAppSpecError {
    InvalidNameError(Yaml),
    InvalidSpecStructureError(String, Yaml),
    MissingCommandError(String, Yaml),
    InvalidWorkingDirectoryError(String, Yaml),
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub(crate) enum ConfigurationSettingsError {
    ConfigurationFileNotFound(String),
    InvalidConfigurationFilePath(String),
    InvalidConfigurationFileContentError(String),
    InvalidConfigurationFileStructureError(Yaml),
    InvalidConfigurationNamespaceError(Yaml),
    InvalidSpecStructuresError(Vec<InvalidAppSpecError>),
}

impl std::fmt::Display for ConfigurationSettingsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&format!("{:?}", self))
    }
}

impl std::error::Error for InvalidAppSpecError {}

impl std::fmt::Display for InvalidAppSpecError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&format!("{:?}", self))
    }
}

impl std::error::Error for ConfigurationSettingsError {}

fn spec_from_hash(
    base_dir: &Path,
    name: &Yaml,
    content: &Yaml,
) -> Result<ProgramSpec, InvalidAppSpecError> {
    let n = name
        .as_str()
        .ok_or(InvalidAppSpecError::InvalidNameError(name.clone()))?;
    let hm = content.as_hash();
    if hm.is_none() {
        return Err(InvalidAppSpecError::InvalidSpecStructureError(
            n.to_owned(),
            content.clone(),
        ));
    }
    let h = hm.unwrap();
    let command_key = Yaml::String("command".to_owned());
    let wd_key = Yaml::String("working_directory".to_owned());
    let command = h.get(&command_key);
    let command_yaml = command
        .ok_or_else(|| InvalidAppSpecError::MissingCommandError(n.to_owned(), content.clone()))?;
    let command_str = command_yaml.as_str().ok_or_else(|| {
        InvalidAppSpecError::MissingCommandError(n.to_owned(), command_yaml.clone())
    })?;

    let path_yaml = h.get(&wd_key);
    let mut path_value = base_dir.to_path_buf();
    if path_yaml.is_some() {
        let p_yaml = path_yaml.unwrap();
        let pys = p_yaml.as_str().ok_or_else(|| {
            InvalidAppSpecError::InvalidWorkingDirectoryError(n.to_owned(), p_yaml.clone())
        })?;
        let p: PathBuf = pys.try_into().map_err(|_p| {
            InvalidAppSpecError::InvalidWorkingDirectoryError(n.to_owned(), p_yaml.clone())
        })?;
        if p.is_absolute() {
            path_value = p;
        } else {
            path_value = path::absolute(base_dir.join(p.as_path())).map_err(|_p| {
                InvalidAppSpecError::InvalidWorkingDirectoryError(n.to_owned(), p_yaml.clone())
            })?;
        }
    }
    Ok(ProgramSpec {
        name: n.to_owned(),
        command: command_str.to_owned(),
        working_directory: path_value.clone(),
        deps: vec![],
    })
}

fn string_to_config(
    base_dir: &Path,
    config_contents: &str,
) -> Result<Configuration, Box<dyn Error>> {
    let yaml_str = YamlLoader::load_from_str(&config_contents);
    if yaml_str.is_err() {
        return Err(Box::new(
            ConfigurationSettingsError::InvalidConfigurationFileContentError(
                config_contents.to_owned(),
            ),
        ));
    }
    let yaml = yaml_str.unwrap();
    let mut oks = Vec::new();
    let mut fails = Vec::new();
    let apps = Yaml::String("apps".to_owned());
    let ns_key = Yaml::String("namespace".to_owned());
    let mut namespace = "devplexer".to_owned();
    for y in yaml.iter() {
        let full_config = y.as_hash().ok_or_else(|| {
            ConfigurationSettingsError::InvalidConfigurationFileStructureError(y.clone())
        })?;
        let ns_val = full_config.get(&ns_key);
        if ns_val.is_some() {
            namespace = ns_val
                .unwrap()
                .as_str()
                .ok_or_else(|| {
                    ConfigurationSettingsError::InvalidConfigurationNamespaceError(
                        ns_val.unwrap().clone(),
                    )
                })?
                .to_owned();
        }
        let app_section = full_config.get(&apps).ok_or_else(|| {
            ConfigurationSettingsError::InvalidConfigurationFileStructureError(y.clone())
        })?;
        let spec_hash = app_section.as_hash().ok_or_else(|| {
            ConfigurationSettingsError::InvalidConfigurationFileStructureError(app_section.clone())
        })?;
        for (k, v) in spec_hash.iter() {
            let newspec = spec_from_hash(base_dir, k, v);
            if newspec.is_ok() {
                oks.push(newspec.unwrap());
            } else {
                fails.push(newspec.unwrap_err());
            }
        }
    }
    if fails.len() > 0 {
        return Err(Box::new(
            ConfigurationSettingsError::InvalidSpecStructuresError(fails),
        ));
    }
    Ok(Configuration {
        namespace: namespace,
        apps: oks,
    })
}

fn load_config(file_path: &Path) -> Result<Configuration, Box<dyn Error>> {
    let p_dir = file_path.parent().unwrap();
    let file_content = std::fs::read_to_string(file_path)?;
    string_to_config(p_dir, &file_content)
}

fn resolve_config_path(
    current_dir: &Path,
    args: &mut std::env::Args,
) -> Result<PathBuf, Box<dyn Error>> {
    if args.len() < 2 {
        Ok(current_dir.join("devplexer.yaml"))
    } else {
        let cfp = &args.nth_back(0).unwrap();
        let pb = PathBuf::from_str(cfp).map_err(|_e| {
            ConfigurationSettingsError::InvalidConfigurationFilePath(cfp.to_owned())
        })?;
        if !pb.is_absolute() {
            Ok(current_dir.join(pb))
        } else {
            Ok(pb)
        }
    }
}

pub(crate) fn try_load_config(
    current_dir: &Path,
    args: &mut std::env::Args,
) -> Result<Configuration, Box<dyn Error>> {
    let full_config_path = resolve_config_path(current_dir, args)?;
    if !full_config_path.exists() {
        return Err(Box::new(
            ConfigurationSettingsError::ConfigurationFileNotFound(
                full_config_path.to_str().unwrap().to_owned(),
            ),
        ));
    }
    load_config(full_config_path.as_path())
}

#[cfg(test)]
mod test {
    use std::{
        path::{Path, PathBuf},
        str::FromStr,
    };

    use crate::config::{ProgramSpec, string_to_config};

    #[test]
    fn test_parse_yaml_config_string() {
        let config_content = r#"
namespace: example-config
apps:
  server:
    command: ls
  server-ui:
    command: echo "blah"
    working_directory: ./ui
"#;
        let base = Path::new("/");
        let config_results = string_to_config(base, config_content).unwrap();
        assert_eq!(
            config_results.apps,
            vec! {
                ProgramSpec {
                    name: "server".to_owned(),
                    command: "ls".to_owned(),
                    working_directory: base.to_path_buf(),
                    deps: vec!{}
                },
                ProgramSpec {
                    name: "server-ui".to_owned(),
                    command: "echo \"blah\"".to_owned(),
                    working_directory: PathBuf::from_str("/ui").unwrap(),
                    deps: vec!{}
                }
            }
        );
        assert_eq!(config_results.namespace, "example-config");
    }
}
