#pragma once

#include <stdint.h>

/* Minimal GL types so we don't need a GL loader or headers from the OBS tree. */
typedef unsigned int GLenum;
typedef unsigned int GLuint;
typedef int GLint;
typedef int GLsizei;
typedef unsigned int GLbitfield;

struct obs_openvr_gl_texture_size {
	GLint width;
	GLint height;
};

/* Loads the GL entry points used below through eglGetProcAddress. Must be
 * called with the OBS graphics context current. Returns 0 on success. */
int obs_openvr_gl_load(void);

void obs_openvr_get_gl_texture_size(GLuint texture, struct obs_openvr_gl_texture_size *out);

/* CPU path: read the whole texture into img (width * height * 4 bytes). */
int obs_openvr_copy_gl_texture(GLuint texture, GLenum format, uint8_t *img);

/* GPU path: blit the (sx, sy, sw, sh) region of src into the whole of dst
 * (dw x dh), using linear filtering when the sizes differ. Restores the
 * framebuffer and texture bindings it touched. Returns 0 on success, or a GL
 * error / framebuffer status code. */
int obs_openvr_blit_gl_texture(GLuint src, GLint sx, GLint sy, GLsizei sw, GLsizei sh,
	GLuint dst, GLsizei dw, GLsizei dh);
