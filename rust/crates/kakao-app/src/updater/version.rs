use super::error::UpdateError;

pub fn version_tuple(value: &str) -> Result<Vec<u32>, UpdateError> {
    let parts: Vec<&str> = value.trim().split('.').collect();
    if parts.is_empty()
        || parts
            .iter()
            .any(|part| part.is_empty() || !part.chars().all(|c| c.is_ascii_digit()))
    {
        return Err(UpdateError::Message(format!(
            "Invalid update version: {value}"
        )));
    }
    Ok(parts.iter().map(|part| part.parse().unwrap_or(0)).collect())
}

pub fn is_newer(candidate: &str, current: &str) -> Result<bool, UpdateError> {
    let mut left = version_tuple(candidate)?;
    let mut right = version_tuple(current)?;
    let width = left.len().max(right.len());
    left.resize(width, 0);
    right.resize(width, 0);
    Ok(left > right)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::VERSION;

    #[test]
    fn newer_version_compares_componentwise() {
        assert!(is_newer("11.0.2", "11.0.1").unwrap());
        assert!(!is_newer("11.0.1", "11.0.1").unwrap());
        assert!(!is_newer("10.9.9", "11.0.1").unwrap());
    }

    #[test]
    fn bumped_package_version_is_newer_than_current() {
        let mut parts: Vec<u32> = VERSION
            .split('.')
            .map(|part| part.parse().expect("VERSION digits"))
            .collect();
        *parts.last_mut().expect("VERSION parts") += 1;
        let newer = parts
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join(".");
        assert!(is_newer(&newer, VERSION).unwrap());
        assert!(!is_newer(VERSION, VERSION).unwrap());
        assert!(!is_newer(VERSION, &newer).unwrap());
    }
}
