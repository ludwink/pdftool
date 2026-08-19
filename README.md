# PDF Tool

- Herramienta simple para extraer páginas de un PDF como imágenes, o dividirlo en varios PDFs.
- Usa PDFium (el motor PDF utilizado por Chromium) vía el crate `pdfium-render`.

## Requisitos

1. Rust, para compilar.
2. Una biblioteca dinámica de PDFium (`libpdfium.so` en Linux).

PDFium debe estar disponible mediante una de estas opciones:

- En el mismo directorio que el ejecutable.
- Indicando su directorio con `--pdfium-lib <directorio>`.

## Instalación

### Cargo

1. Compilar e instalar (o reinstalar)

```shell
cargo install --path .
```

> - Lo instala en la carpeta `~/.cargo/bin`
> - Ver ejecutables instalados `cargo install --list`

### Binario

1. Compilar

```shell
cargo build -r
```

> El ejecutable se genera en `target/release/pdftool`

2. Instalar

Linux. Para el usuario

```shell
install -Dm755 target/release/pdftool ~/.local/bin/pdftool
```

## Desintalar

1. Binario

- Cargo `cargo uninstall pdftool`
- Instalado para el usuario en Linux `rm ~/.local/bin/pdftool`

2. Eliminar la biblioteca dinámica, por ejemplo `libpdfium.so`.
