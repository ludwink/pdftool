# PDF Tool

- Herramienta simple para extraer un rango de páginas en un nuevo PDF o,
- para exportarlas como imágenes.
- Usa PDFium (el motor PDF utilizado por Chromium).

## Requisitos

1. Rust, para compilar.
2. Biblioteca dinámica de PDFium (`.so`, `.dll`).

PDFium debe estar disponible mediante una de estas opciones:

- En el mismo directorio que el ejecutable.
- En la ruta actual de trabajo.
- Instalado en el Sistema Operativo.

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

## Ejemplos de uso

```bash
# Extrae un rango de páginas en un nuevo PDF
pdftool document.pdf split --from 10 --to 20                    #./output/document - 10-20.pdf

# Extrae un rango de páginas en formato WebP
pdftool document.pdf images --from 5 --format webp --prefix ""  #./output/005.webp, 006.webp, ...
```

## Desintalar

1. Binario

- Cargo `cargo uninstall pdftool`
- Instalado para el usuario en Linux `rm ~/.local/bin/pdftool`

2. Opcionalmente, eliminar la biblioteca dinámica, por ejemplo `libpdfium.so`.
