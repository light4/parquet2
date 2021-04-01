// Licensed to the Apache Software Foundation (ASF) under one
// or more contributor license agreements.  See the NOTICE file
// distributed with this work for additional information
// regarding copyright ownership.  The ASF licenses this file
// to you under the Apache License, Version 2.0 (the
// "License"); you may not use this file except in compliance
// with the License.  You may obtain a copy of the License at
//
//   http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing,
// software distributed under the License is distributed on an
// "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied.  See the License for the
// specific language governing permissions and limitations
// under the License.

//! Contains codec interface and supported codec implementations.
//!
//! See [`CompressionCodec`](crate::basic::CompressionCodec) enum for all available compression
//! algorithms.
//!
//! # Example
//!
//! ```no_run
//! use parquet::{basic::CompressionCodec, compression::create_codec};
//!
//! let mut codec = match create_codec(CompressionCodec::SNAPPY) {
//!     Ok(Some(codec)) => codec,
//!     _ => panic!(),
//! };
//!
//! let data = vec![b'p', b'a', b'r', b'q', b'u', b'e', b't'];
//! let mut compressed = vec![];
//! codec.compress(&data[..], &mut compressed).unwrap();
//!
//! let mut output = vec![];
//! codec.decompress(&compressed[..], &mut output).unwrap();
//!
//! assert_eq!(output, data);
//! ```

pub use parquet_format::CompressionCodec;

use crate::errors::{ParquetError, Result};

/// Parquet compression codec interface.
pub trait Codec: std::fmt::Debug {
    /// Compresses data stored in slice `input_buf` and writes the compressed result
    /// to `output_buf`.
    /// Note that you'll need to call `clear()` before reusing the same `output_buf`
    /// across different `compress` calls.
    fn compress(&mut self, input_buf: &[u8], output_buf: &mut Vec<u8>) -> Result<()>;

    /// Decompresses data stored in slice `input_buf` and writes output to `output_buf`.
    /// Returns the total number of bytes written.
    fn decompress(&mut self, input_buf: &[u8], output_buf: &mut Vec<u8>) -> Result<usize>;
}

/// Given the compression type `codec`, returns a codec used to compress and decompress
/// bytes for the compression type.
/// This returns `None` if the codec type is `UNCOMPRESSED`.
pub fn create_codec(codec: &CompressionCodec) -> Result<Option<Box<dyn Codec>>> {
    use CompressionCodec::*;
    match codec {
        #[cfg(feature = "brotli")]
        Brotli => Ok(Some(Box::new(BrotliCodec::new()))),
        #[cfg(feature = "gzip")]
        Gzip => Ok(Some(Box::new(GZipCodec::new()))),
        #[cfg(feature = "snappy")]
        Snappy => Ok(Some(Box::new(SnappyCodec::new()))),
        #[cfg(feature = "lz4")]
        Lz4 => Ok(Some(Box::new(Lz4Codec::new()))),
        #[cfg(feature = "zstd")]
        Zstd => Ok(Some(Box::new(ZstdCodec::new()))),
        Uncompressed => Ok(None),
        _ => Err(general_err!(
            "CompressionCodec {:?} is not installed",
            codec
        )),
    }
}

#[cfg(feature = "snappy")]
mod snappy_codec {
    use snap::raw::{decompress_len, max_compress_len, Decoder, Encoder};

    use crate::compression::Codec;
    use crate::errors::Result;

    /// Codec for Snappy compression format.
    #[derive(Debug)]
    pub struct SnappyCodec {
        decoder: Decoder,
        encoder: Encoder,
    }

    impl SnappyCodec {
        /// Creates new Snappy compression codec.
        pub(crate) fn new() -> Self {
            Self {
                decoder: Decoder::new(),
                encoder: Encoder::new(),
            }
        }
    }

    impl Codec for SnappyCodec {
        fn decompress(&mut self, input_buf: &[u8], output_buf: &mut Vec<u8>) -> Result<usize> {
            let len = decompress_len(input_buf)?;
            output_buf.resize(len, 0);
            self.decoder
                .decompress(input_buf, output_buf)
                .map_err(|e| e.into())
        }

        fn compress(&mut self, input_buf: &[u8], output_buf: &mut Vec<u8>) -> Result<()> {
            let output_buf_len = output_buf.len();
            let required_len = max_compress_len(input_buf.len());
            output_buf.resize(output_buf_len + required_len, 0);
            let n = self
                .encoder
                .compress(input_buf, &mut output_buf[output_buf_len..])?;
            output_buf.truncate(output_buf_len + n);
            Ok(())
        }
    }
}
#[cfg(feature = "snappy")]
pub use snappy_codec::*;

#[cfg(feature = "gzip")]
mod gzip_codec {

    use std::io::{Read, Write};

    use flate2::{read, write, Compression};

    use crate::compression::Codec;
    use crate::errors::Result;

    /// Codec for GZIP compression algorithm.
    #[derive(Debug)]
    pub struct GZipCodec {}

    impl GZipCodec {
        /// Creates new GZIP compression codec.
        pub(crate) fn new() -> Self {
            Self {}
        }
    }

    impl Codec for GZipCodec {
        fn decompress(&mut self, input_buf: &[u8], output_buf: &mut Vec<u8>) -> Result<usize> {
            let mut decoder = read::GzDecoder::new(input_buf);
            decoder.read_to_end(output_buf).map_err(|e| e.into())
        }

        fn compress(&mut self, input_buf: &[u8], output_buf: &mut Vec<u8>) -> Result<()> {
            let mut encoder = write::GzEncoder::new(output_buf, Compression::default());
            encoder.write_all(input_buf)?;
            encoder.try_finish().map_err(|e| e.into())
        }
    }
}
#[cfg(feature = "gzip")]
pub use gzip_codec::*;

#[cfg(feature = "brotli")]
mod brotli_codec {

    use std::io::{Read, Write};

    use crate::compression::Codec;
    use crate::errors::Result;

    const BROTLI_DEFAULT_BUFFER_SIZE: usize = 4096;
    const BROTLI_DEFAULT_COMPRESSION_QUALITY: u32 = 1; // supported levels 0-9
    const BROTLI_DEFAULT_LG_WINDOW_SIZE: u32 = 22; // recommended between 20-22

    /// Codec for Brotli compression algorithm.
    #[derive(Debug)]
    pub struct BrotliCodec {}

    impl BrotliCodec {
        /// Creates new Brotli compression codec.
        pub(crate) fn new() -> Self {
            Self {}
        }
    }

    impl Codec for BrotliCodec {
        fn decompress(&mut self, input_buf: &[u8], output_buf: &mut Vec<u8>) -> Result<usize> {
            brotli::Decompressor::new(input_buf, BROTLI_DEFAULT_BUFFER_SIZE)
                .read_to_end(output_buf)
                .map_err(|e| e.into())
        }

        fn compress(&mut self, input_buf: &[u8], output_buf: &mut Vec<u8>) -> Result<()> {
            let mut encoder = brotli::CompressorWriter::new(
                output_buf,
                BROTLI_DEFAULT_BUFFER_SIZE,
                BROTLI_DEFAULT_COMPRESSION_QUALITY,
                BROTLI_DEFAULT_LG_WINDOW_SIZE,
            );
            encoder.write_all(input_buf)?;
            encoder.flush().map_err(|e| e.into())
        }
    }
}
#[cfg(feature = "brotli")]
pub use brotli_codec::*;

#[cfg(feature = "lz4")]
mod lz4_codec {
    use std::io::{Read, Write};

    use crate::compression::Codec;
    use crate::errors::Result;

    const LZ4_BUFFER_SIZE: usize = 4096;

    /// Codec for LZ4 compression algorithm.
    #[derive(Debug)]
    pub struct Lz4Codec {}

    impl Lz4Codec {
        /// Creates new LZ4 compression codec.
        pub(crate) fn new() -> Self {
            Self {}
        }
    }

    impl Codec for Lz4Codec {
        fn decompress(&mut self, input_buf: &[u8], output_buf: &mut Vec<u8>) -> Result<usize> {
            let mut decoder = lz4::Decoder::new(input_buf)?;
            let mut buffer: [u8; LZ4_BUFFER_SIZE] = [0; LZ4_BUFFER_SIZE];
            let mut total_len = 0;
            loop {
                let len = decoder.read(&mut buffer)?;
                if len == 0 {
                    break;
                }
                total_len += len;
                output_buf.write_all(&buffer[0..len])?;
            }
            Ok(total_len)
        }

        fn compress(&mut self, input_buf: &[u8], output_buf: &mut Vec<u8>) -> Result<()> {
            let mut encoder = lz4::EncoderBuilder::new().build(output_buf)?;
            let mut from = 0;
            loop {
                let to = std::cmp::min(from + LZ4_BUFFER_SIZE, input_buf.len());
                encoder.write_all(&input_buf[from..to])?;
                from += LZ4_BUFFER_SIZE;
                if from >= input_buf.len() {
                    break;
                }
            }
            encoder.finish().1.map_err(|e| e.into())
        }
    }
}
#[cfg(feature = "lz4")]
pub use lz4_codec::*;

#[cfg(feature = "zstd")]
mod zstd_codec {
    use std::io::{self, Write};

    use crate::compression::Codec;
    use crate::errors::Result;

    /// Codec for Zstandard compression algorithm.
    #[derive(Debug)]
    pub struct ZstdCodec {}

    impl ZstdCodec {
        /// Creates new Zstandard compression codec.
        pub(crate) fn new() -> Self {
            Self {}
        }
    }

    /// CompressionCodec level (1-21) for ZSTD. Choose 1 here for better compression speed.
    const ZSTD_COMPRESSION_LEVEL: i32 = 1;

    impl Codec for ZstdCodec {
        fn decompress(&mut self, input_buf: &[u8], output_buf: &mut Vec<u8>) -> Result<usize> {
            let mut decoder = zstd::Decoder::new(input_buf)?;
            match io::copy(&mut decoder, output_buf) {
                Ok(n) => Ok(n as usize),
                Err(e) => Err(e.into()),
            }
        }

        fn compress(&mut self, input_buf: &[u8], output_buf: &mut Vec<u8>) -> Result<()> {
            let mut encoder = zstd::Encoder::new(output_buf, ZSTD_COMPRESSION_LEVEL)?;
            encoder.write_all(input_buf)?;
            match encoder.finish() {
                Ok(_) => Ok(()),
                Err(e) => Err(e.into()),
            }
        }
    }
}
#[cfg(any(feature = "zstd"))]
pub use zstd_codec::*;

#[cfg(test)]
mod tests {
    use super::*;

    fn test_roundtrip(c: CompressionCodec, data: &[u8]) {
        let mut c1 = create_codec(&c).unwrap().unwrap();
        let mut c2 = create_codec(&c).unwrap().unwrap();

        // Compress with c1
        let mut compressed = Vec::new();
        let mut decompressed = Vec::new();
        c1.compress(data, &mut compressed)
            .expect("Error when compressing");

        // Decompress with c2
        let mut decompressed_size = c2
            .decompress(compressed.as_slice(), &mut decompressed)
            .expect("Error when decompressing");
        assert_eq!(data.len(), decompressed_size);
        decompressed.truncate(decompressed_size);
        assert_eq!(data, decompressed.as_slice());

        compressed.clear();

        // Compress with c2
        c2.compress(data, &mut compressed)
            .expect("Error when compressing");

        // Decompress with c1
        decompressed_size = c1
            .decompress(compressed.as_slice(), &mut decompressed)
            .expect("Error when decompressing");
        assert_eq!(data.len(), decompressed_size);
        decompressed.truncate(decompressed_size);
        assert_eq!(data, decompressed.as_slice());
    }

    fn test_codec(c: CompressionCodec) {
        let sizes = vec![100, 10000, 100000];
        for size in sizes {
            let data = (0..size).map(|x| (x % 255) as u8).collect::<Vec<_>>();
            test_roundtrip(c, &data);
        }
    }

    #[test]
    fn test_codec_snappy() {
        test_codec(CompressionCodec::Snappy);
    }

    #[test]
    fn test_codec_gzip() {
        test_codec(CompressionCodec::Gzip);
    }

    #[test]
    fn test_codec_brotli() {
        test_codec(CompressionCodec::Brotli);
    }

    #[test]
    fn test_codec_lz4() {
        test_codec(CompressionCodec::Lz4);
    }

    #[test]
    fn test_codec_zstd() {
        test_codec(CompressionCodec::Zstd);
    }
}