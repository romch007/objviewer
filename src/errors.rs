use color_eyre::eyre::eyre;

pub trait WrapGlErrorExt<T> {
    fn wrap_gl_error(self) -> color_eyre::Result<T>;
}

impl<T> WrapGlErrorExt<T> for Result<T, String> {
    fn wrap_gl_error(self) -> color_eyre::Result<T> {
        self.map_err(|msg| eyre!("gl error: {msg}"))
    }
}
