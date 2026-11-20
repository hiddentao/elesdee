//! Pairing QR images.

use std::io;

use image::ImageEncoder;

/// Renders a pairing ticket as a PNG image.
///
/// The image is produced in memory and is never written to disk or logged.
///
/// # Errors
///
/// Returns an error when the ticket is too long for a QR code or the PNG
/// encoder fails.
pub fn ticket_png(ticket: &str) -> io::Result<Vec<u8>> {
    let code = qrcode::QrCode::new(ticket.as_bytes())
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error.to_string()))?;
    let image = code.render::<image::Luma<u8>>().build();
    let (width, height) = (image.width(), image.height());
    let mut output = Vec::new();
    image::codecs::png::PngEncoder::new(&mut output)
        .write_image(image.as_raw(), width, height, image::ExtendedColorType::L8)
        .map_err(|error| io::Error::other(error.to_string()))?;
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_ticket_renders_as_a_png() {
        let png = ticket_png("elesdee-pairtest").expect("render png");
        assert!(png.starts_with(b"\x89PNG\r\n\x1a\n"));
        assert!(png.len() > 100);
    }
}
