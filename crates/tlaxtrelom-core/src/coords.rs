// SPDX-License-Identifier: MIT
// Copyright (c) 2026 angiraulene1-create
//! Coordenadas de dos capas para un mundo abierto de gran alcance.
//!
//! El techo real de un mundo "de años luz" no es el motor sino la aritmética:
//! el `f32` tiene 24 bits de mantisa, así que más allá de 2^14 m (~16 km) su ULP
//! supera el milímetro y más allá de 2^20 m (~1000 km) sumar medio IPD a la
//! posición del ojo ya no cambia el número. A 1e11 m un ULP son 8 km: el LOD del
//! streamer deja de distinguir clusters vecinos. Por eso separamos dos capas:
//!
//! - **Anclaje global** (`WorldOrigin.anchor`, `f64`): dónde está el origen de
//!   render en metros. Con `f64` la resolución milimétrica llega a ~9e9 m del
//!   anclaje, sobra para un sistema planetario. Para cruzar una galaxia el
//!   contenido se divide en *sectores*: cada sector rebasa el origen y el
//!   anclaje registra dónde cae.
//! - **Offset local** (`LocalPose.offset`, `f32`): posición relativa al origen,
//!   siempre de unos pocos cientos de metros. Todo lo que se dibuja, las
//!   matrices y el LOD se calculan contra este offset, nunca contra el anclaje.
//!
//! Rebase: cuando el offset del jugador supera `REBASE_THRESHOLD`, se pliega su
//! parte entera al anclaje `f64` y el offset queda cerca de cero. El rebase es
//! determinista (redondeo IEEE, `f64 += f64`), así que dos runs con el mismo
//! input producen el mismo `(anchor, offset)`.

/// Distancia del offset a partir de la cual se pliega al anclaje. 1024 m deja
/// holgura de precisión `f32` de sub-milímetro en todo el volumen jugable.
pub const REBASE_THRESHOLD: f32 = 1024.0;

/// Posición global del origen de render, en metros, con precisión `f64`.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct WorldOrigin {
    pub anchor: [f64; 3],
}

impl WorldOrigin {
    pub const ZERO: Self = Self { anchor: [0.0; 3] };

    pub fn new(anchor: [f64; 3]) -> Self {
        Self { anchor }
    }

    /// Convierte una posición global (sector) a un offset `f32` relativo a este
    /// origen. Si la resta no cabe finita en `f32` (anclaje y global absurdamente
    /// separados), devuelve `None` en vez de un valor basura.
    pub fn to_local(&self, global: [f64; 3]) -> Option<[f32; 3]> {
        let mut out = [0.0f32; 3];
        for (o, (g, a)) in out.iter_mut().zip(global.iter().zip(self.anchor)) {
            let d = g - a;
            if !d.is_finite() || d.abs() > f64::from(f32::MAX) {
                return None;
            }
            *o = d as f32;
        }
        Some(out)
    }

    /// Suma un desplazamiento global al anclaje. Usado por el rebase.
    fn shift(&mut self, delta: [f64; 3]) {
        for (a, d) in self.anchor.iter_mut().zip(delta) {
            *a += d;
        }
    }
}

/// Pose local del jugador: offset `f32` respecto del origen + orientación.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct LocalPose {
    pub offset: [f32; 3],
    pub yaw: f32,
}

impl LocalPose {
    pub fn is_finite(&self) -> bool {
        self.offset.iter().all(|v| v.is_finite()) && self.yaw.is_finite()
    }

    /// `true` si algún eje del offset pasó el umbral de rebase.
    pub fn should_rebase(&self) -> bool {
        self.offset
            .iter()
            .any(|v| v.abs() >= REBASE_THRESHOLD || !v.is_finite())
    }
}

/// Pliega la parte entera de los ejes que pasaron el umbral al anclaje.
///
/// Es una función pura: no muta. Sólo se pliega un eje cuando su valor absoluto
/// supera `REBASE_THRESHOLD`, así que la altura de ojos (`offset[1] ≈ 1.7`)
/// queda intacta en el frame local y no contamina el anclaje. El residuo de un
/// eje plegado cae en `[-0.5, 0.5]`, lo que evita deriva. Un offset no finito se
/// neutraliza a cero (el anclaje no puede absorber NaN).
pub fn rebase(mut origin: WorldOrigin, mut pose: LocalPose) -> (WorldOrigin, LocalPose) {
    let mut delta = [0.0f64; 3];
    for (d, o) in delta.iter_mut().zip(pose.offset.iter_mut()) {
        if !o.is_finite() {
            *o = 0.0;
            continue;
        }
        if o.abs() >= REBASE_THRESHOLD {
            *d = o.round() as f64;
            *o -= *d as f32;
        }
    }
    origin.shift(delta);
    (origin, pose)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn intent_offset(x: f32, z: f32) -> LocalPose {
        LocalPose {
            offset: [x, 0.0, z],
            yaw: 0.0,
        }
    }

    #[test]
    fn to_local_subtracts_anchor() {
        let origin = WorldOrigin::new([1_000_000.0, 0.0, -2_000_000.0]);
        let local = origin
            .to_local([1_000_012.5, 1.7, -1_999_988.0])
            .expect("cabe en f32");
        assert_eq!(local, [12.5, 1.7, 12.0]);
    }

    #[test]
    fn to_local_rejects_absurd_distance() {
        let origin = WorldOrigin::new([0.0, 0.0, 0.0]);
        // 1e30 sí cabe en f32 (max ≈ 3.4e38); 1e40 no, y al cast a f32 sería Inf.
        assert!(origin.to_local([1e30, 0.0, 0.0]).is_some());
        assert!(origin.to_local([1e40, 0.0, 0.0]).is_none());
    }

    #[test]
    fn should_rebase_triggers_past_threshold() {
        assert!(!intent_offset(1000.0, 0.0).should_rebase());
        assert!(intent_offset(REBASE_THRESHOLD, 0.0).should_rebase());
        assert!(intent_offset(-4000.0, 0.0).should_rebase());
        assert!(intent_offset(f32::NAN, 0.0).should_rebase());
    }

    #[test]
    fn rebase_folds_integer_part_into_anchor() {
        let origin = WorldOrigin::ZERO;
        let pose = intent_offset(1500.4, -2200.6);
        let (origin, pose) = rebase(origin, pose);
        // 1500.4.round() = 1500, -2200.6.round() = -2201 (alejado de cero).
        assert_eq!(origin.anchor, [1500.0, 0.0, -2201.0]);
        // Residuo = offset - delta, en [-0.5, 0.5]: 1500.4-1500=0.4,
        // -2200.6 - (-2201) = 0.4.
        assert!((pose.offset[0] - 0.4).abs() < 1e-3);
        assert!((pose.offset[2] - 0.4).abs() < 1e-3);
        assert!(pose.offset.iter().all(|v| v.abs() < 0.5 + 1e-3));
    }

    #[test]
    fn rebase_neutralizes_non_finite_offset() {
        let origin = WorldOrigin::new([10.0, 0.0, 0.0]);
        let pose = intent_offset(f32::NAN, f32::INFINITY);
        let (origin, pose) = rebase(origin, pose);
        assert!(pose.is_finite());
        assert_eq!(pose.offset, [0.0, 0.0, 0.0]);
        assert_eq!(origin.anchor, [10.0, 0.0, 0.0]);
    }

    /// Los umbrales que citan `docs/01-architecture.md` y la documentación de
    /// este módulo, medidos sobre el `f32` real en vez de afirmados.
    #[test]
    fn the_documented_f32_thresholds_are_real() {
        // A 2^14 m (~16 km) el ULP del f32 supera el milímetro. Sobre una
        // potencia de dos, la separación con el siguiente representable es
        // exactamente `x * EPSILON`.
        let ten_six_km = 2.0f32.powi(14);
        let ulp = ten_six_km * f32::EPSILON;
        assert!(ulp > 0.001, "ULP a 16 km = {ulp} m");

        // A 2^20 m (~1000 km) sumar medio IPD no cambia el número: el estéreo
        // de coordenadas absolutas se convierte en mono.
        let far = 2.0f32.powi(20);
        let half_ipd = 0.0315f32;
        assert_eq!(far + half_ipd, far, "el umbral de colapso del IPD se movió");

        // Con dos capas, la misma posición global sigue dando ojos distintos:
        // el anclaje `f64` absorbe la magnitud y el offset conserva la separación.
        let origin = WorldOrigin::new([f64::from(far), 0.0, 0.0]);
        let left = origin
            .to_local([f64::from(far) - f64::from(half_ipd), 0.0, 0.0])
            .expect("cabe");
        let right = origin
            .to_local([f64::from(far) + f64::from(half_ipd), 0.0, 0.0])
            .expect("cabe");
        assert!(
            right[0] - left[0] > 0.06,
            "estéreo colapsado: {left:?} {right:?}"
        );
    }

    #[test]
    fn ipd_survives_at_galactic_anchor() {
        // El escenario que el f32 absoluto no puede: cámara a 1e11 m (≈6.7 UA).
        // Con origen flotante el offset queda cerca de cero y el medio-IPD
        // (0.0315 m) sigue siendo distinguible.
        let origin = WorldOrigin::new([1e11, 0.0, 0.0]);
        let local = origin
            .to_local([1e11 + 0.5 * 0.063, 0.0, 0.0])
            .expect("cabe");
        // El offset es la posición del ojo izquierdo; el derecho está a +IPD.
        let half = 0.063 * 0.5;
        let left = local[0] - half;
        let right = local[0] + half;
        assert!(
            right > left,
            "el estéreo colapsó: left={left} right={right}"
        );
        assert!((right - left - 0.063).abs() < 1e-5);
    }

    #[test]
    fn rebase_keeps_eye_height_local() {
        // Y = altura de ojos (≈1.7) nunca debe plegarse al anclaje, aunque X/Z
        // pasen el umbral: si lo hiciera, round(1.7)=2 corrompería la pose.
        let origin = WorldOrigin::ZERO;
        let pose = LocalPose {
            offset: [2000.0, 1.7, -3000.4],
            yaw: 0.5,
        };
        let (origin, pose) = rebase(origin, pose);
        assert_eq!(origin.anchor, [2000.0, 0.0, -3000.0]);
        assert!(
            (pose.offset[1] - 1.7).abs() < 1e-6,
            "la altura de ojos se plegó"
        );
        assert!((pose.offset[0]).abs() < 1e-3);
        assert!((pose.offset[2] - (-0.4)).abs() < 1e-3);
        assert_eq!(pose.yaw, 0.5);
    }

    #[test]
    fn repeated_rebase_is_deterministic() {
        // Dos runs con la misma secuencia de offsets deben dar el mismo
        // (anchor, offset): el selftest depende de esto vía digest().
        fn run(seed: f32) -> (WorldOrigin, LocalPose) {
            let mut origin = WorldOrigin::ZERO;
            let mut pose = LocalPose::default();
            let mut x = seed;
            for _ in 0..500 {
                x += 5.3;
                pose.offset = [x, 0.0, x * 0.5];
                if pose.should_rebase() {
                    let (o, p) = rebase(origin, pose);
                    origin = o;
                    pose = p;
                }
            }
            (origin, pose)
        }
        let a = run(0.0);
        let b = run(0.0);
        assert_eq!(a, b);
        assert!(a.1.offset.iter().all(|v| v.abs() < REBASE_THRESHOLD + 1.0));
    }
}
