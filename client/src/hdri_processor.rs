use bevy::{
    asset::{
        io::{AssetReaderError, AsyncWriteExt, Writer},
        processor::{Process, ProcessContext, ProcessError},
        RenderAssetUsages,
    },
    image::{ImageFormat, ImageFormatSetting, ImageLoader, ImageLoaderSettings, ImageSampler},
    prelude::*,
    reflect::TypePath,
};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
pub struct HdriProcessorSettings {
    pub resolution: u32,
}

impl Default for HdriProcessorSettings {
    fn default() -> Self {
        Self { resolution: 512 }
    }
}

#[derive(TypePath)]
pub struct HdriProcessor;

impl Process for HdriProcessor {
    type Settings = HdriProcessorSettings;
    type OutputLoader = ImageLoader;

    async fn process(
        &self,
        context: &mut ProcessContext<'_>,
        settings: &Self::Settings,
        writer: &mut Writer,
    ) -> Result<ImageLoaderSettings, ProcessError> {
        let mut bytes = Vec::new();
        context
            .asset_reader()
            .read_to_end(&mut bytes)
            .await
            .map_err(|err| ProcessError::AssetReaderError {
                path: context.path().clone_owned(),
                err: err.into(),
            })?;
        if context.path().path().extension().and_then(|ext| ext.to_str()) != Some("hdr") {
            return Err(ProcessError::AssetReaderError {
                path: context.path().clone_owned(),
                err: AssetReaderError::Io(
                    std::io::Error::new(
                        std::io::ErrorKind::InvalidInput,
                        "unsupported HDR source extension",
                    )
                    .into(),
                ),
            });
        }
        let output = ktx2conv::convert_hdr_bytes_to_ktx2(&bytes, settings.resolution.max(1))
            .map_err(|err| ProcessError::AssetTransformError(Box::new(err)))?;
        writer
            .write_all(&output)
            .await
            .map_err(|err| ProcessError::AssetWriterError {
                path: context.path().clone_owned(),
                err: err.into(),
            })?;
        Ok(ImageLoaderSettings {
            format: ImageFormatSetting::Format(ImageFormat::Ktx2),
            is_srgb: false,
            sampler: ImageSampler::Default,
            asset_usage: RenderAssetUsages::default(),
            texture_format: None,
            array_layout: None,
        })
    }
}

pub struct HdriProcessorPlugin;

impl Plugin for HdriProcessorPlugin {
    fn build(&self, app: &mut App) {
        app.register_asset_processor(HdriProcessor)
            .set_default_asset_processor::<HdriProcessor>("hdr");
    }
}
