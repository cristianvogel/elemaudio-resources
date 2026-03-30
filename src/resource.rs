use std::any::Any;
use std::collections::{HashMap, HashSet};
use std::fmt::{Debug, Formatter};
use std::sync::Arc;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ResourceId(String);

impl ResourceId {
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl AsRef<str> for ResourceId {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

#[derive(Clone)]
pub struct AudioBuffer {
    pub samples: Arc<[f32]>,
    pub sample_rate: u32,
    pub channels: u16,
}

impl AudioBuffer {
    pub fn mono(samples: impl Into<Arc<[f32]>>, sample_rate: u32) -> Self {
        Self {
            samples: samples.into(),
            sample_rate,
            channels: 1,
        }
    }

    pub fn frames(&self) -> usize {
        self.samples.len() / self.channels as usize
    }
}

impl Debug for AudioBuffer {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AudioBuffer")
            .field("sample_rate", &self.sample_rate)
            .field("channels", &self.channels)
            .field("samples", &self.samples.len())
            .finish()
    }
}

#[derive(Clone)]
pub enum Resource {
    Audio(AudioBuffer),
    F32(Arc<[f32]>),
    F64(Arc<[f64]>),
    Bytes(Arc<[u8]>),
    Text(Arc<str>),
    Any(Arc<dyn Any + Send + Sync>),
}

impl Debug for Resource {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Audio(buffer) => buffer.fmt(f),
            Self::F32(samples) => f.debug_tuple("F32").field(&samples.len()).finish(),
            Self::F64(samples) => f.debug_tuple("F64").field(&samples.len()).finish(),
            Self::Bytes(bytes) => f.debug_tuple("Bytes").field(&bytes.len()).finish(),
            Self::Text(text) => f.debug_tuple("Text").field(&text.len()).finish(),
            Self::Any(_) => f.write_str("Any(<opaque>)"),
        }
    }
}

impl Resource {
    pub fn audio(buffer: AudioBuffer) -> Self {
        Self::Audio(buffer)
    }

    pub fn bytes(data: impl Into<Arc<[u8]>>) -> Self {
        Self::Bytes(data.into())
    }

    pub fn as_audio(&self) -> Option<&AudioBuffer> {
        match self {
            Self::Audio(buffer) => Some(buffer),
            _ => None,
        }
    }

    pub fn as_bytes(&self) -> Option<&[u8]> {
        match self {
            Self::Bytes(bytes) => Some(bytes.as_ref()),
            _ => None,
        }
    }

    pub fn kind(&self) -> &'static str {
        match self {
            Self::Audio(_) => "audio",
            Self::F32(_) => "f32",
            Self::F64(_) => "f64",
            Self::Bytes(_) => "bytes",
            Self::Text(_) => "text",
            Self::Any(_) => "any",
        }
    }
}

#[derive(Default, Clone, Debug)]
pub struct ResourceManager {
    resources: HashMap<ResourceId, Resource>,
}

impl ResourceManager {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn snapshot(&self) -> Vec<(ResourceId, Resource)> {
        self.resources
            .iter()
            .map(|(id, resource)| (id.clone(), resource.clone()))
            .collect()
    }

    pub fn get(&self, id: impl AsRef<str>) -> Option<&Resource> {
        self.resources.get(&ResourceId::new(id.as_ref()))
    }

    pub fn get_cloned(&self, id: impl AsRef<str>) -> Option<Resource> {
        self.get(id).cloned()
    }

    pub fn insert(
        &mut self,
        id: impl AsRef<str>,
        resource: Resource,
    ) -> Result<Option<Resource>, String> {
        let id = ResourceId::new(id.as_ref());
        Ok(self.resources.insert(id, resource))
    }

    pub fn add(&mut self, id: impl AsRef<str>, resource: Resource) -> Result<(), String> {
        let id = ResourceId::new(id.as_ref());
        if self.resources.contains_key(&id) {
            return Err(format!("resource already exists: {}", id.as_str()));
        }
        self.resources.insert(id, resource);
        Ok(())
    }

    pub fn remove(&mut self, id: impl AsRef<str>) -> Result<Resource, String> {
        let id = ResourceId::new(id.as_ref());
        self.resources
            .remove(&id)
            .ok_or_else(|| format!("resource not found: {}", id.as_str()))
    }

    pub fn rename(&mut self, from: impl AsRef<str>, to: impl AsRef<str>) -> Result<(), String> {
        let from = ResourceId::new(from.as_ref());
        let to = ResourceId::new(to.as_ref());
        if self.resources.contains_key(&to) {
            return Err(format!("resource already exists: {}", to.as_str()));
        }
        let resource = self
            .resources
            .remove(&from)
            .ok_or_else(|| format!("resource not found: {}", from.as_str()))?;
        self.resources.insert(to, resource);
        Ok(())
    }

    pub fn prune_except<I, S>(&mut self, keep: I) -> Vec<(ResourceId, Resource)>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let keep: HashSet<String> = keep.into_iter().map(|id| id.as_ref().to_string()).collect();
        let mut removed = Vec::new();
        self.resources.retain(|id, resource| {
            if keep.contains(id.as_str()) {
                true
            } else {
                removed.push((id.clone(), resource.clone()));
                false
            }
        });
        removed
    }
}
