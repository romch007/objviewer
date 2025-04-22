mod errors;

use crate::errors::WrapGlErrorExt;
use color_eyre::eyre::{bail, Context, ContextCompat};
use glam::{Mat4, Vec3};
use glow::HasContext;
use sdl3::{event::Event, keyboard::Keycode, mouse::MouseButton};

const WINDOW_WIDTH: u32 = 1280;
const WINDOW_HEIGHT: u32 = 720;

const FOV: f32 = 45.0;
const INITIAL_CAMERA_RADIUS: f32 = 5.0;

const OBJ_VERTEX_SHADER_SOURCE: &str = r#"
    #version 330 core
    in vec3 position;
    
    uniform mat4 mvp;

    void main() {
        gl_Position = mvp * vec4(position, 1.0);
    }
"#;

const AXIS_VERTEX_SHADER_SOURCE: &str = r#"
    #version 330 core

    layout(location = 0) in vec3 position;
    layout(location = 1) in vec3 color;

    uniform mat4 mvp;

    out vec3 vertex_color;

    void main() {
        gl_Position = mvp * vec4(position, 1.0);
        vertex_color = color;
    }
"#;

const OBJ_FRAGMENT_SHADER_SOURCE: &str = r#"
    #version 330 core

    out vec4 vertex_color;

    void main() {
        vertex_color = vec4(0.41, 0.41, 0.41, 1.0);
    }
"#;

const AXIS_FRAGMENT_SHADER_SOURCE: &str = r#"
    #version 330 core

    in vec3 vertex_color;
    out vec4 color;

    void main() {
        color = vec4(vertex_color, 1.0);
    }
"#;

const EDGE_FRAGMENT_SHADER_SOURCE: &str = r#"
    #version 330 core

    out vec4 vertex_color;

    void main() {
        vertex_color = vec4(0.8, 0.8, 0.8, 1.0);
    }
"#;

fn main() -> color_eyre::Result<()> {
    color_eyre::install()?;

    let obj = wavefront::Obj::from_file("teapot.obj").wrap_err("cannot parse Wavefront file")?;
    let obj_triangles = obj.triangles().collect::<Vec<_>>();
    let mut vertex_data = Vec::with_capacity(obj_triangles.len() * 3);

    // TODO: more rusty
    for triangle in obj_triangles {
        for vertex in triangle {
            vertex_data.push(vertex.position()[0]);
            vertex_data.push(vertex.position()[1]);
            vertex_data.push(vertex.position()[2]);
        }
    }

    let edge_data = extract_edges_from_triangles(&vertex_data);

    let sdl_context = sdl3::init().wrap_err("cannot init SDL3")?;
    let video_subsystem = sdl_context.video().wrap_err("cannot init video")?;

    println!("using sdl3 {}", sdl3::version::version());
    println!("video driver: {}", video_subsystem.current_video_driver());

    let gl_attr = video_subsystem.gl_attr();
    gl_attr.set_context_profile(sdl3::video::GLProfile::Core);
    gl_attr.set_context_version(3, 2);

    let window = video_subsystem
        .window("OBJ viewer", WINDOW_WIDTH, WINDOW_HEIGHT)
        .position_centered()
        .opengl()
        .build()
        .wrap_err("cannot create window")?;

    let _gl_context = window
        .gl_create_context()
        .wrap_err("cannot create OpenGL context")?;

    let gl = unsafe {
        glow::Context::from_loader_function(|s| {
            if let Some(proc_addr) = video_subsystem.gl_get_proc_address(s) {
                proc_addr as *const _
            } else {
                std::ptr::null()
            }
        })
    };

    unsafe { gl.enable(glow::DEPTH_TEST) };

    // OBJ setup
    let obj_program =
        create_shader_program(&gl, OBJ_VERTEX_SHADER_SOURCE, OBJ_FRAGMENT_SHADER_SOURCE)?;
    let (obj_vao, _obj_vbo) = create_obj_buffers(&gl, &vertex_data)?;

    // Edges setup
    let edges_program =
        create_shader_program(&gl, OBJ_VERTEX_SHADER_SOURCE, EDGE_FRAGMENT_SHADER_SOURCE)?;
    let (edges_vao, _edges_vbo) = create_edge_buffers(&gl, &edge_data)?;

    // Axis setup
    let axis_program =
        create_shader_program(&gl, AXIS_VERTEX_SHADER_SOURCE, AXIS_FRAGMENT_SHADER_SOURCE)?;
    let (axis_vao, _axis_vbo) = create_axis_buffer(&gl)?;

    let mut camera_theta = 0.0f32;
    let mut camera_phi = 0.0f32;
    let mut camera_zoom_factor = 1.0f32;
    let camera_target = Vec3::ZERO;
    let camera_up = Vec3::Y;

    let mut mouse_last_x = 0.0f32;
    let mut mouse_last_y = 0.0f32;
    let mut mouse_is_dragging = false;

    let mut event_pump = sdl_context
        .event_pump()
        .wrap_err("cannot create event pump")?;

    'running: loop {
        for event in event_pump.poll_iter() {
            match event {
                Event::Quit { .. }
                | Event::KeyDown {
                    keycode: Some(Keycode::Escape),
                    ..
                } => break 'running,
                Event::MouseButtonDown {
                    x,
                    y,
                    mouse_btn: MouseButton::Left,
                    ..
                } => {
                    mouse_is_dragging = true;
                    mouse_last_x = x;
                    mouse_last_y = y;
                }
                Event::MouseButtonUp {
                    mouse_btn: MouseButton::Left,
                    ..
                } => {
                    mouse_is_dragging = false;
                }
                Event::MouseMotion { x, y, .. } => {
                    if mouse_is_dragging {
                        let dx = x - mouse_last_x;
                        let dy = y - mouse_last_y;

                        camera_theta += dx * 0.005;
                        camera_phi += dy * 0.005;

                        camera_phi = camera_phi
                            .clamp(-std::f32::consts::FRAC_PI_2, std::f32::consts::FRAC_PI_2);

                        mouse_last_x = x;
                        mouse_last_y = y;
                    }
                }
                Event::KeyDown {
                    keycode: Some(Keycode::Left),
                    ..
                } => {
                    camera_theta -= 0.1;
                }
                Event::KeyDown {
                    keycode: Some(Keycode::Right),
                    ..
                } => {
                    camera_theta += 0.1;
                }
                Event::KeyDown {
                    keycode: Some(Keycode::Up),
                    ..
                } => {
                    camera_phi += 0.1;
                    if camera_phi > std::f32::consts::FRAC_PI_2 {
                        camera_phi = std::f32::consts::FRAC_PI_2;
                    }
                }
                Event::KeyDown {
                    keycode: Some(Keycode::Down),
                    ..
                } => {
                    camera_phi -= 0.1;
                    if camera_phi < -std::f32::consts::FRAC_PI_2 {
                        camera_phi = -std::f32::consts::FRAC_PI_2;
                    }
                }
                Event::MouseWheel { y, .. } => {
                    if y > 0.0 {
                        camera_zoom_factor -= 0.1;
                    } else {
                        camera_zoom_factor += 0.1;
                    }
                    camera_zoom_factor = camera_zoom_factor.clamp(0.1, 10.0);
                }
                _ => {}
            };
        }

        let camera_radius = INITIAL_CAMERA_RADIUS * camera_zoom_factor;

        let x = camera_radius * camera_phi.cos() * camera_theta.cos();
        let y = camera_radius * camera_phi.sin();
        let z = camera_radius * camera_phi.cos() * camera_theta.sin();

        let camera_position = Vec3::new(x, y, z);

        unsafe { gl.clear_color(0.5, 0.5, 0.5, 1.0) };
        unsafe { gl.clear(glow::COLOR_BUFFER_BIT | glow::DEPTH_BUFFER_BIT) };

        let model = Mat4::IDENTITY;
        let view = Mat4::look_at_rh(camera_position, camera_target, camera_up);
        let projection = Mat4::perspective_rh_gl(
            FOV.to_radians(),
            WINDOW_WIDTH as f32 / WINDOW_HEIGHT as f32,
            0.1,
            100.0,
        );
        let mvp = projection * view * model;

        draw_obj(
            &gl,
            obj_vao,
            obj_program,
            &mvp,
            (vertex_data.len() / 3) as i32,
        )?;

        draw_edges(&gl, edges_vao, edges_program, &mvp, edge_data.len() as i32)?;

        unsafe { gl.disable(glow::DEPTH_TEST) };

        draw_axes(&gl, axis_vao, axis_program, &mvp)?;

        unsafe { gl.enable(glow::DEPTH_TEST) };

        window.gl_swap_window();
    }

    Ok(())
}

fn extract_edges_from_triangles(vertex_data: &[f32]) -> Vec<f32> {
    let mut edge_data = Vec::new();

    for triangle in vertex_data.chunks_exact(9) {
        let v0 = &triangle[0..3];
        let v1 = &triangle[3..6];
        let v2 = &triangle[6..9];

        edge_data.extend_from_slice(v0);
        edge_data.extend_from_slice(v1);

        edge_data.extend_from_slice(v1);
        edge_data.extend_from_slice(v2);

        edge_data.extend_from_slice(v2);
        edge_data.extend_from_slice(v0);
    }

    edge_data
}

fn create_shader_program(
    gl: &glow::Context,
    vertex_shader_source: &str,
    fragment_shader_source: &str,
) -> color_eyre::Result<glow::Program> {
    unsafe {
        let vertex_shader = gl.create_shader(glow::VERTEX_SHADER).wrap_gl_error()?;
        gl.shader_source(vertex_shader, vertex_shader_source);
        gl.compile_shader(vertex_shader);

        if gl.get_shader_compile_status(vertex_shader) {
            bail!("vertex shader failed to compile: {}", gl.get_shader_info_log(vertex_shader));
        }

        let fragment_shader = gl.create_shader(glow::FRAGMENT_SHADER).wrap_gl_error()?;
        gl.shader_source(fragment_shader, fragment_shader_source);
        gl.compile_shader(fragment_shader);

        if gl.get_shader_compile_status(fragment_shader) {
            bail!("fragment shader failed to compile: {}", gl.get_shader_info_log(fragment_shader));
        }

        let program = gl.create_program().wrap_gl_error()?;
        gl.attach_shader(program, vertex_shader);
        gl.attach_shader(program, fragment_shader);
        gl.link_program(program);
        if gl.get_program_link_status(program) {
            bail!("program failed to link: {}", gl.get_program_info_log(program));
        }

        Ok(program)
    }
}

fn create_obj_buffers(
    gl: &glow::Context,
    vertex_data: &[f32],
) -> color_eyre::Result<(glow::NativeVertexArray, glow::NativeBuffer)> {
    unsafe {
        let vao = gl.create_vertex_array().wrap_gl_error()?;
        gl.bind_vertex_array(Some(vao));

        let vbo = gl.create_buffer().wrap_gl_error()?;
        gl.bind_buffer(glow::ARRAY_BUFFER, Some(vbo));
        gl.buffer_data_u8_slice(
            glow::ARRAY_BUFFER,
            bytemuck::cast_slice(vertex_data),
            glow::STATIC_DRAW,
        );

        gl.vertex_attrib_pointer_f32(
            0,
            3,
            glow::FLOAT,
            false,
            3 * std::mem::size_of::<f32>() as i32,
            0,
        );
        gl.enable_vertex_attrib_array(0);

        Ok((vao, vbo))
    }
}

fn create_edge_buffers(
    gl: &glow::Context,
    edge_data: &[f32],
) -> color_eyre::Result<(glow::NativeVertexArray, glow::NativeBuffer)> {
    unsafe {
        let vao = gl.create_vertex_array().wrap_gl_error()?;
        gl.bind_vertex_array(Some(vao));

        let vbo = gl.create_buffer().wrap_gl_error()?;
        gl.bind_buffer(glow::ARRAY_BUFFER, Some(vbo));
        gl.buffer_data_u8_slice(
            glow::ARRAY_BUFFER,
            bytemuck::cast_slice(edge_data),
            glow::STATIC_DRAW,
        );

        gl.vertex_attrib_pointer_f32(
            0,
            3,
            glow::FLOAT,
            false,
            3 * std::mem::size_of::<f32>() as i32,
            0,
        );
        gl.enable_vertex_attrib_array(0);

        Ok((vao, vbo))
    }
}

const AXIS_DATA: [f32; 36] = [
    0.0, 0.0, 0.0, 1.0, 0.0, 0.0, // start point, color
    1.0, 0.0, 0.0, 1.0, 0.0, 0.0, // end point, color
    // Y-axis (Green)
    0.0, 0.0, 0.0, 0.0, 1.0, 0.0, // start point, color
    0.0, 1.0, 0.0, 0.0, 1.0, 0.0, // end point, color
    // Z-axis (Blue)
    0.0, 0.0, 0.0, 0.0, 0.0, 1.0, // start point, color
    0.0, 0.0, 1.0, 0.0, 0.0, 1.0, // end point, color
];

fn create_axis_buffer(gl: &glow::Context) -> color_eyre::Result<(glow::NativeVertexArray, glow::NativeBuffer)> {
    unsafe {
        let vbo = gl.create_buffer().wrap_gl_error()?;
        gl.bind_buffer(glow::ARRAY_BUFFER, Some(vbo));
        gl.buffer_data_u8_slice(
            glow::ARRAY_BUFFER,
            bytemuck::cast_slice(&AXIS_DATA),
            glow::STATIC_DRAW,
        );

        let vao = gl.create_vertex_array().wrap_gl_error()?;
        gl.bind_vertex_array(Some(vao));

        gl.vertex_attrib_pointer_f32(
            0,
            3,
            glow::FLOAT,
            false,
            6 * std::mem::size_of::<f32>() as i32,
            0,
        );
        gl.vertex_attrib_pointer_f32(
            1,
            3,
            glow::FLOAT,
            false,
            6 * std::mem::size_of::<f32>() as i32,
            3 * std::mem::size_of::<f32>() as i32,
        );
        gl.enable_vertex_attrib_array(0);
        gl.enable_vertex_attrib_array(1);

        Ok((vao, vbo))
    }
}

fn draw_obj(
    gl: &glow::Context,
    vao: glow::NativeVertexArray,
    program: glow::Program,
    mvp: &Mat4,
    triangles_count: i32,
) -> color_eyre::Result<()> {
    unsafe {
        gl.use_program(Some(program));

        let mvp_location = gl.get_uniform_location(program, "mvp").wrap_err("no location for uniform")?;

        gl.uniform_matrix_4_f32_slice(Some(&mvp_location), false, mvp.to_cols_array().as_slice());

        gl.bind_vertex_array(Some(vao));
        gl.draw_arrays(glow::TRIANGLES, 0, triangles_count);

        Ok(())
    }
}

fn draw_edges(
    gl: &glow::Context,
    vao: glow::NativeVertexArray,
    program: glow::Program,
    mvp: &Mat4,
    line_count: i32,
) -> color_eyre::Result<()> {
    unsafe {
        gl.line_width(2.0);

        gl.use_program(Some(program));

        let mvp_location = gl.get_uniform_location(program, "mvp").wrap_err("no location for uniform")?;

        gl.uniform_matrix_4_f32_slice(Some(&mvp_location), false, mvp.to_cols_array().as_slice());

        gl.bind_vertex_array(Some(vao));
        gl.draw_arrays(glow::LINES, 0, line_count);

        Ok(())
    }
}

fn draw_axes(gl: &glow::Context, vao: glow::NativeVertexArray, program: glow::Program, mvp: &Mat4) -> color_eyre::Result<()> {
    unsafe {
        gl.use_program(Some(program));

        let mvp_location = gl.get_uniform_location(program, "mvp").wrap_err("no location for uniform")?;

        gl.uniform_matrix_4_f32_slice(Some(&mvp_location), false, mvp.to_cols_array().as_slice());

        gl.bind_vertex_array(Some(vao));
        gl.draw_arrays(glow::LINES, 0, 6);

        Ok(())
    }
}
