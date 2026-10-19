use super::super::diagram::{RasterCamera, ViewBox};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Camera {
    pub scale: f64,
    pub tx: f64,
    pub ty: f64,
}
impl Camera {
    pub fn fit(b: ViewBox, size: [f64; 2]) -> Self {
        let scale = ((size[0] - 96.).max(1.) / b.width)
            .min((size[1] - 96.).max(1.) / b.height)
            .min(1.5);
        Self {
            scale,
            tx: (size[0] - b.width * scale) / 2. - b.x * scale,
            ty: (size[1] - b.height * scale) / 2. - b.y * scale,
        }
    }
    pub fn zoom(&mut self, point: [f64; 2], next: f64, minimum: f64) {
        let scale = next.clamp(minimum, 8.);
        let ratio = scale / self.scale;
        self.tx = point[0] - (point[0] - self.tx) * ratio;
        self.ty = point[1] - (point[1] - self.ty) * ratio;
        self.scale = scale;
    }
    pub fn pan(&mut self, delta: [f64; 2]) {
        self.tx += delta[0];
        self.ty += delta[1];
    }
    pub fn resize(&mut self, old: [f64; 2], new: [f64; 2]) {
        self.pan([(new[0] - old[0]) / 2., (new[1] - old[1]) / 2.]);
    }
    pub fn raster(self, size: [f64; 2], backing_scale: f64) -> RasterCamera {
        RasterCamera {
            viewport: ViewBox {
                x: -self.tx / self.scale,
                y: -self.ty / self.scale,
                width: size[0] / self.scale,
                height: size[1] / self.scale,
            },
            zoom: self.scale,
            backing_scale,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn fit_zoom_resize_preserve_diagram_points() {
        let b = ViewBox {
            x: -20.,
            y: 30.,
            width: 300.,
            height: 100.,
        };
        let mut c = Camera::fit(b, [900., 700.]);
        assert_eq!(c.scale, 1.5);
        let point = [140., 220.];
        let diagram = [(point[0] - c.tx) / c.scale, (point[1] - c.ty) / c.scale];
        c.zoom(point, 100., 0.1);
        assert_eq!(c.scale, 8.);
        assert!(((point[0] - c.tx) / c.scale - diagram[0]).abs() < 1e-9);
        c.resize([900., 700.], [700., 500.]);
        assert!(((40. - c.tx) / c.scale - diagram[0]).abs() < 1e-9);
        let r = c.raster([700., 500.], 2.);
        assert_eq!(r.viewport.width * r.zoom * r.backing_scale, 1400.);
    }
    #[test]
    fn huge_fit_can_zoom_below_point_one() {
        let c = Camera::fit(
            ViewBox {
                x: 0.,
                y: 0.,
                width: 16000.,
                height: 16000.,
            },
            [100., 100.],
        );
        assert!(c.scale < 0.1);
    }
}
