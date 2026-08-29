use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand, ValueEnum};
use image::DynamicImage;
use indicatif::{ProgressBar, ProgressStyle};
use jpegxl_rs::{
    encode::{ColorEncoding, EncoderFrame, EncoderResult, EncoderSpeed},
    encoder_builder,
};
use pdfium_render::prelude::*;
use webpx::{Encoder, Unstoppable};

/// # pdftool
/// Herramienta simple para extraer un rango de páginas en un nuevo PDF o,
/// para exportarlas como imágenes.
/// Usa `PDFium`, el motor PDF utilizado por Chromium.
#[derive(Parser)]
#[command(name = "pdftool", version, about, long_about = None)]
struct Cli {
    /// Ruta al archivo PDF de entrada
    input: PathBuf,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Extraer un rango de páginas como imágenes (JPEG XL o WebP)
    Images {
        /// Página inicial (1-indexado, inclusive)
        #[arg(long, default_value_t = 1)]
        from: u16,

        /// Página final (1-indexado, inclusive). Por defecto, la última página.
        #[arg(long)]
        to: Option<u16>,

        /// Formato de salida
        #[arg(long, value_enum, default_value_t = ImageFormatArg::Jxl)]
        format: ImageFormatArg,

        /// Resolución de renderizado en puntos por pulgada (DPI).
        /// 150 = borrador rápido, 300 = calidad estándar de impresión/OCR,
        /// 600 = alta calidad (archivos más grandes y más lento).
        #[arg(long, default_value_t = 300, value_parser = clap::value_parser!(u32).range(1..=2400))]
        dpi: u32,

        /// Calidad WebP (1-100). Ignorado si el formato es JXL (que no tiene pérdida).
        #[arg(long, default_value_t = 80, value_parser = clap::value_parser!(u8).range(1..=100))]
        quality: u8,

        /// Directorio de salida
        #[arg(long, default_value = "./output")]
        output_dir: PathBuf,

        /// Prefijo de los archivos generados. Por defecto, el nombre del PDF sin extensión.
        #[arg(long)]
        prefix: Option<String>,
    },

    /// Extraer un rango de páginas como un único PDF nuevo
    Split {
        /// Página inicial (1-indexado, inclusive)
        #[arg(long, default_value_t = 1)]
        from: u16,

        /// Página final (1-indexado, inclusive). Por defecto, la última página.
        #[arg(long)]
        to: Option<u16>,

        /// Directorio de salida
        #[arg(long, default_value = "./output")]
        output_dir: PathBuf,

        /// Prefijo de los archivos generados. Por defecto, el nombre del PDF sin extensión.
        #[arg(long)]
        prefix: Option<String>,
    },
}

#[derive(Copy, Clone, ValueEnum)]
enum ImageFormatArg {
    Jxl,
    Webp,
}

impl ImageFormatArg {
    /// Retorna la extensión de archivo correspondiente al formato.
    fn extension(self) -> &'static str {
        match self {
            Self::Jxl => "jxl",
            Self::Webp => "webp",
        }
    }

    /// Guarda la imagen en disco de forma atómica: escribe primero en un archivo
    /// temporal en el mismo directorio y solo lo renombra al destino final si
    /// la codificación se completó con éxito. Así se evita dejar un archivo
    /// corrupto/parcial si el proceso falla o es interrumpido a mitad de escritura.
    fn save(self, image: &DynamicImage, path: &Path, quality: u8) -> Result<()> {
        let tmp_path = tmp_path_for(path);

        let write_result = (|| -> Result<()> {
            // `bitmap.as_image()` produce un DynamicImage::ImageRgba8. Se toma
            // prestado su búfer sin convertir ni copiar los píxeles.
            let rgba = image
                .as_rgba8()
                .context("PDFium no devolvió una imagen RGBA8")?;

            match self {
                Self::Jxl => {
                    // `encode()` usa tres canales por defecto. Para conservar el
                    // alfa hay que describir explícitamente un frame RGBA.
                    let frame = EncoderFrame::new(rgba.as_raw().as_slice()).num_channels(4);
                    let mut encoder = encoder_builder()
                        .has_alpha(true)
                        // La codificación matemáticamente lossless no puede usar
                        // la transformación interna a XYB: debe conservar el
                        // perfil/color de los píxeles de entrada.
                        .uses_original_profile(true)
                        .color_encoding(ColorEncoding::Srgb)
                        .lossless(true)
                        .speed(EncoderSpeed::Glacier)
                        .build()
                        .context("no se pudo inicializar el codificador JPEG XL")?;

                    // El render de PDFium ya contiene muestras de 8 bits; pasarlas
                    // a RGBA16 no agregaría información y duplicaría la memoria.
                    let encoded: EncoderResult<u8> = encoder
                        .encode_frame(&frame, rgba.width(), rgba.height())
                        .context("no se pudo codificar JPEG XL")?;

                    std::fs::write(&tmp_path, encoded.data)
                        .with_context(|| format!("no se pudo escribir {}", tmp_path.display()))?;
                }

                Self::Webp => {
                    let encoded = Encoder::new_rgba(rgba.as_raw(), rgba.width(), rgba.height())
                        .quality(f32::from(quality))
                        .encode(Unstoppable)
                        .map_err(|err| anyhow::anyhow!("no se pudo codificar WebP: {err}"))?;

                    std::fs::write(&tmp_path, encoded)
                        .with_context(|| format!("no se pudo escribir {}", tmp_path.display()))?;
                }
            }
            Ok(())
        })();

        // Si algo falló durante la escritura, se limpia el temporal antes de propagar el error.
        if let Err(err) = write_result {
            let _ = std::fs::remove_file(&tmp_path);
            return Err(err);
        }
        std::fs::rename(&tmp_path, path).with_context(|| {
            format!(
                "no se pudo renombrar {} a {}",
                tmp_path.display(),
                path.display()
            )
        })?;

        Ok(())
    }
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let pdfium = init_pdfium()?;

    let document = pdfium
        .load_pdf_from_file(&cli.input, None)
        .with_context(|| format!("no se pudo abrir el PDF: {}", cli.input.display()))?;

    let page_count = usize::try_from(document.pages().len())
        .context("el PDF reportó un número de páginas inválido")?;
    if page_count == 0 {
        bail!("el PDF no tiene páginas");
    }

    let stem = cli
        .input
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("documento");

    match cli.command {
        Command::Images {
            from,
            to,
            format,
            dpi,
            quality,
            output_dir,
            prefix,
        } => {
            let (start, end) = resolve_range(from, to, page_count)?;
            let prefix = prefix.as_deref().unwrap_or(stem);
            let options = ImageExtractOptions {
                format,
                dpi,
                quality,
                output_dir,
                prefix: prefix.to_string(),
            };
            extract_images(&document, start, end, &options)?;
        }
        Command::Split {
            from,
            to,
            output_dir,
            prefix,
        } => {
            let (start, end) = resolve_range(from, to, page_count)?;
            let prefix = prefix.as_deref().unwrap_or(stem);
            split_range(&pdfium, &document, start, end, &output_dir, prefix)?;
        }
    }

    Ok(())
}

/// Intenta enlazar con `PDFium`:
/// primero busca en el directorio donde está instalado el ejecutable,
/// después en el directorio donde fue llamado el ejecutable
/// por último, busca si está instalado en el sistema operativo.
fn init_pdfium() -> Result<Pdfium> {
    // obtiene la ruta donde se almacena el binario
    // solo ruta, no incluye nombre del archivo ("pdfium")
    let exe_dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(Path::to_path_buf))
        .context("no se pudo obtener la ruta padre del ejecutable.")?;

    // agrega el nombre de la libreria para formar la ruta completa ("pdfium")
    let file_path = Pdfium::pdfium_platform_library_name_at_path(&exe_dir);
    let file_path_current_working_directory = Pdfium::pdfium_platform_library_name_at_path("./");

    let bindings = Pdfium::bind_to_library(file_path)
        .or_else(|_| Pdfium::bind_to_library(file_path_current_working_directory))
        .or_else(|_| Pdfium::bind_to_system_library())
        .with_context(|| {
            format!(
                "no se encontró la librería nativa de PDFium junto al ejecutable ({}) \
                ni donde fue llamada la herramienta ni en el sistema operativo. \
                Descárgala desde https://github.com/bblanchon/pdfium-binaries \
                y colócala junto al ejecutable o en tu directorio de trabajo. \
                instala PDFium en tu sistema operativo.",
                exe_dir.display()
            )
        })?;

    Ok(Pdfium::new(bindings))
}

/// Convierte el rango 1-indexado e inclusivo dado por el usuario
/// al rango 0-indexado que espera pdfium-render, validando sus límites.
fn resolve_range(from: u16, to: Option<u16>, page_count: usize) -> Result<(usize, usize)> {
    let from = from as usize;
    let to = to.map_or(page_count, |v| v as usize);

    if from == 0 || to == 0 {
        bail!("las páginas se numeran desde 1, no desde 0");
    }
    if from > to {
        bail!("--from ({from}) no puede ser mayor que --to ({to})");
    }
    if to > page_count {
        bail!("el PDF solo tiene {page_count} páginas, pero --to = {to}");
    }

    Ok((from - 1, to - 1)) // Rango 0-indexado, inclusive
}

/// Formatea el nombre del archivo de salida omitiendo el separador si el prefijo está vacío.
fn build_filename(prefix: &str, identifier: &str, ext: &str) -> String {
    if prefix.is_empty() {
        format!("{identifier}.{ext}")
    } else {
        format!("{prefix} - {identifier}.{ext}")
    }
}

/// Genera una ruta temporal en el mismo directorio que `path`, para que el
/// `rename` final sea atómico (mismo sistema de archivos).
fn tmp_path_for(path: &Path) -> PathBuf {
    let file_name = path.file_name().map_or_else(
        || ".output.tmp".to_string(),
        |n| format!(".{}.tmp", n.to_string_lossy()),
    );
    path.with_file_name(file_name)
}

/// Crea y configura la barra de progreso para la consola.
fn progress_bar(len: u64, msg: &'static str) -> ProgressBar {
    let pb = ProgressBar::new(len);
    pb.set_style(
        ProgressStyle::with_template("{msg} [{bar:40}] {pos}/{len}")
            .unwrap()
            .progress_chars("=> "),
    );
    pb.set_message(msg);
    pb
}

/// Opciones para la extracción de páginas como imágenes.
struct ImageExtractOptions {
    format: ImageFormatArg,
    dpi: u32,
    quality: u8,
    output_dir: PathBuf,
    prefix: String,
}

/// Extraer un rango de páginas como imágenes (JPEG XL o WebP)
fn extract_images(
    document: &PdfDocument,
    start: usize,
    end: usize,
    options: &ImageExtractOptions,
) -> Result<()> {
    let ImageExtractOptions {
        format,
        dpi,
        quality,
        output_dir,
        prefix,
    } = options;

    std::fs::create_dir_all(output_dir)
        .with_context(|| format!("no se pudo crear el directorio {}", output_dir.display()))?;

    let total = (end - start + 1) as u64;
    let pb = progress_bar(total, "Renderizando páginas");

    for index in start..=end {
        let page_index = i32::try_from(index).context("índice de página fuera de rango")?;
        let page = document.pages().get(page_index)?;
        // El tamaño de página en PDFium se especifica en puntos tipográficos (1/72 de pulgada).
        let scale = (*dpi as f32) / 72.0;

        let render_config = PdfRenderConfig::new().scale_page_by_factor(scale);

        let bitmap = page
            .render_with_config(&render_config)
            .with_context(|| format!("fallo al renderizar la página {}", index + 1))?;

        let image = bitmap
            .as_image()
            .with_context(|| format!("fallo al convertir a imagen la página {}", index + 1))?;

        let page_suffix = format!("{:03}", index + 1);
        let file_name = build_filename(prefix, &page_suffix, format.extension());
        let out_path = output_dir.join(file_name);

        format.save(&image, &out_path, *quality)?;

        pb.inc(1);
    }

    pb.finish_with_message("Listo");
    println!(
        "Extraídas {} páginas ({}-{}) a {}",
        total,
        start + 1,
        end + 1,
        output_dir.display()
    );

    Ok(())
}

/// Extrae un rango de páginas a un nuevo PDF copiando los objetos
/// directamente de la estructura del PDF.
fn split_range(
    pdfium: &Pdfium,
    document: &PdfDocument,
    start: usize,
    end: usize,
    output_dir: &Path,
    prefix: &str,
) -> Result<()> {
    std::fs::create_dir_all(output_dir)?;

    let mut new_doc = pdfium.create_new_pdf()?;
    let range_start = i32::try_from(start).context("página inicial fuera de rango")?;
    let range_end = i32::try_from(end).context("página final fuera de rango")?;
    let range = range_start..=range_end;
    new_doc
        .pages_mut()
        .copy_page_range_from_document(document, range, 0)
        .context("no se pudo copiar el rango de páginas")?;

    let range_suffix = format!("{}-{}", start + 1, end + 1);
    let file_name = build_filename(prefix, &range_suffix, "pdf");
    let out_path = output_dir.join(file_name);
    let tmp_path = tmp_path_for(&out_path);

    let save_result = new_doc
        .save_to_file(&tmp_path)
        .with_context(|| format!("no se pudo guardar {}", tmp_path.display()));

    if save_result.is_err() {
        let _ = std::fs::remove_file(&tmp_path);
        save_result?;
    }

    std::fs::rename(&tmp_path, &out_path).with_context(|| {
        format!(
            "no se pudo renombrar {} a {}",
            tmp_path.display(),
            out_path.display()
        )
    })?;

    println!(
        "Generado {} con las páginas {}-{}",
        out_path.display(),
        start + 1,
        end + 1
    );

    Ok(())
}
