//! A skin as a Bedrock client sends it, turned into render options.

use image::RgbaImage;

use crate::{
    AnimatedType, Error, Geometry, RenderOptions, Skin, is_empty, parse_geometry,
    parse_resource_patch, texture_from_rgba,
};

/// A skin as a Bedrock client sends it - in the login packet, a PlayerList
/// entry or a PlayerSkin packet: images as raw RGBA with their sizes
/// alongside, and the model as JSON. The field names follow the protocol's,
/// so a proxy or bot can fill one straight from the packet.
///
/// ```
/// use bedrock_skin::{View, WireSkin};
/// let pixels = vec![255u8; 64 * 64 * 4];
/// let skin = WireSkin {
///     skin_data: &pixels,
///     skin_width: 64,
///     skin_height: 64,
///     geometry: b"null",
///     resource_patch: br#"{"geometry":{"default":"geometry.humanoid.customSlim"}}"#,
///     ..WireSkin::default()
/// }
/// .decode()?;
/// assert_eq!(skin.identifier, "geometry.humanoid.customSlim");
/// let png = skin.options().view(View::Avatar).size(128).render_png()?;
/// # let _ = png;
/// # Ok::<(), bedrock_skin::Error>(())
/// ```
///
/// See docs/skin-data.md for what each field holds.
#[derive(Clone, Debug, Default)]
pub struct WireSkin<'a> {
    /// The skin's pixels, raw non-premultiplied RGBA, width*height*4 bytes.
    /// Required.
    pub skin_data: &'a [u8],
    pub skin_width: u32,
    pub skin_height: u32,
    /// The cape's pixels in the same form; empty for no cape.
    pub cape_data: &'a [u8],
    pub cape_width: u32,
    pub cape_height: u32,
    /// SkinGeometryData. Empty or the literal `null` - what a client sends
    /// for a built-in model - draws the default model.
    pub geometry: &'a [u8],
    /// SkinResourcePatch: it names which entry of the geometry the skin
    /// uses, wide or slim among them.
    pub resource_patch: &'a [u8],
    /// The skin's animation list. A persona skin's face, and some of its
    /// body, are textured by these.
    pub animations: Vec<WireAnimation<'a>>,
}

/// One entry of a skin's animation list: its image as raw RGBA, and its type
/// as the protocol numbers it.
#[derive(Clone, Copy, Debug, Default)]
pub struct WireAnimation<'a> {
    pub animation_type: u32,
    pub data: &'a [u8],
    pub width: u32,
    pub height: u32,
}

/// A [`WireSkin`] decoded: the images it carried, its geometry, and the
/// entry its resource patch names. [`DecodedSkin::options`] renders it.
#[derive(Clone, Debug)]
pub struct DecodedSkin {
    pub texture: RgbaImage,
    pub cape: Option<RgbaImage>,
    /// Empty for a built-in model, which renders the default model.
    pub geometry: Vec<Geometry>,
    /// The entry the resource patch names; empty when it names none.
    pub identifier: String,
    /// The animation images the renderer draws, by type.
    pub animated: Vec<(AnimatedType, RgbaImage)>,
}

impl WireSkin<'_> {
    /// Decodes the wire fields: the images wrapped, the geometry parsed, the
    /// entry the resource patch names picked out, and the animation images
    /// kept so persona heads draw.
    ///
    /// A resource patch that does not parse is not an error - the patch only
    /// picks an entry, and without it the entry with the most cubes is used,
    /// as for an empty patch. Malformed geometry and images whose data does
    /// not match their size are errors. An animation of a type the renderer
    /// does not draw is skipped.
    pub fn decode(&self) -> Result<DecodedSkin, Error> {
        let image = |what: &str, data: &[u8], w: u32, h: u32| {
            texture_from_rgba(data.to_vec(), w, h)
                .map_err(|e| Error::Pixels(format!("{what}: {e}")))
        };
        let texture = image("skin", self.skin_data, self.skin_width, self.skin_height)?;
        let cape = if self.cape_data.is_empty() {
            None
        } else {
            Some(image(
                "cape",
                self.cape_data,
                self.cape_width,
                self.cape_height,
            )?)
        };
        let geometry = if is_empty(self.geometry) {
            Vec::new()
        } else {
            parse_geometry(self.geometry)?
        };
        let identifier = if self.resource_patch.is_empty() {
            String::new()
        } else {
            parse_resource_patch(self.resource_patch).map_or_else(|_| String::new(), |p| p.default)
        };
        let mut animated = Vec::new();
        for a in &self.animations {
            let img = image(
                &format!("animation {}", a.animation_type),
                a.data,
                a.width,
                a.height,
            )?;
            if let Some(kind) = AnimatedType::from_protocol(a.animation_type) {
                animated.push((kind, img));
            }
        }
        Ok(DecodedSkin {
            texture,
            cape,
            geometry,
            identifier,
            animated,
        })
    }

    /// The invisibility detector's view of the same fields: the texture and
    /// its geometry. It does not need the cape or the animations.
    pub fn skin(&self) -> Result<Skin, Error> {
        let texture = texture_from_rgba(self.skin_data.to_vec(), self.skin_width, self.skin_height)
            .map_err(|e| Error::Pixels(format!("skin: {e}")))?;
        let geometry = (!is_empty(self.geometry)).then_some(self.geometry);
        Ok(Skin::new(texture, geometry))
    }
}

impl DecodedSkin {
    /// Render options for the skin, its cape, model and animation images set;
    /// set the view, size and the rest on the result.
    pub fn options(&self) -> RenderOptions<'_> {
        let mut o = RenderOptions::new(&self.texture)
            .geometry(&self.geometry)
            .identifier(self.identifier.clone());
        o.cape = self.cape.as_ref();
        for (kind, img) in &self.animated {
            o = o.animated(*kind, img);
        }
        o
    }
}
