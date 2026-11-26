#include "gl-copy.h"

#include <dlfcn.h>
#include <stddef.h>
#include <stdio.h>

#define GL_NO_ERROR 0
#define GL_TEXTURE_2D 0x0DE1
#define GL_TEXTURE_WIDTH 0x1000
#define GL_TEXTURE_HEIGHT 0x1001
#define GL_TEXTURE_BINDING_2D 0x8069
#define GL_UNSIGNED_BYTE 0x1401
#define GL_COLOR_BUFFER_BIT 0x00004000
#define GL_NEAREST 0x2600
#define GL_LINEAR 0x2601
#define GL_READ_FRAMEBUFFER 0x8CA8
#define GL_DRAW_FRAMEBUFFER 0x8CA9
#define GL_READ_FRAMEBUFFER_BINDING 0x8CAA
#define GL_DRAW_FRAMEBUFFER_BINDING 0x8CA6
#define GL_COLOR_ATTACHMENT0 0x8CE0
#define GL_FRAMEBUFFER_COMPLETE 0x8CD5

typedef void *(*get_proc_fn)(const char *);

static struct {
	int loaded;
	GLenum (*GetError)(void);
	void (*GetIntegerv)(GLenum, GLint *);
	void (*BindTexture)(GLenum, GLuint);
	void (*GetTexLevelParameteriv)(GLenum, GLint, GLenum, GLint *);
	void (*GetTexImage)(GLenum, GLint, GLenum, GLenum, void *);
	void (*GenFramebuffers)(GLsizei, GLuint *);
	void (*BindFramebuffer)(GLenum, GLuint);
	void (*FramebufferTexture2D)(GLenum, GLenum, GLenum, GLuint, GLint);
	GLenum (*CheckFramebufferStatus)(GLenum);
	void (*BlitFramebuffer)(GLint, GLint, GLint, GLint, GLint, GLint, GLint, GLint, GLbitfield, GLenum);
	GLuint read_fbo;
	GLuint draw_fbo;
} gl;

#define LOAD(name)                                                        \
	do {                                                              \
		*(void **)&gl.name = get_proc("gl" #name);                \
		if (!gl.name) {                                           \
			fprintf(stderr, "obs-openvr: missing gl" #name "\n"); \
			return -1;                                        \
		}                                                         \
	} while (0)

int obs_openvr_gl_load(void)
{
	if (gl.loaded)
		return 0;

	/* OBS on Linux renders through EGL (X11 and Wayland). Mesa and NVIDIA
	 * both return core entry points from eglGetProcAddress. */
	void *egl = dlopen("libEGL.so.1", RTLD_LAZY | RTLD_NOLOAD);
	if (!egl)
		egl = dlopen("libEGL.so.1", RTLD_LAZY);
	if (!egl) {
		fprintf(stderr, "obs-openvr: could not load libEGL.so.1: %s\n", dlerror());
		return -1;
	}
	get_proc_fn get_proc = (get_proc_fn)dlsym(egl, "eglGetProcAddress");
	if (!get_proc) {
		fprintf(stderr, "obs-openvr: eglGetProcAddress not found\n");
		return -1;
	}

	LOAD(GetError);
	LOAD(GetIntegerv);
	LOAD(BindTexture);
	LOAD(GetTexLevelParameteriv);
	LOAD(GetTexImage);
	LOAD(GenFramebuffers);
	LOAD(BindFramebuffer);
	LOAD(FramebufferTexture2D);
	LOAD(CheckFramebufferStatus);
	LOAD(BlitFramebuffer);

	gl.loaded = 1;
	return 0;
}

static void clear_errors(void)
{
	for (int i = 0; i < 16 && gl.GetError() != GL_NO_ERROR; i++)
		;
}

void obs_openvr_get_gl_texture_size(GLuint texture, struct obs_openvr_gl_texture_size *out)
{
	out->width = 0;
	out->height = 0;
	if (obs_openvr_gl_load() != 0)
		return;
	GLint prev = 0;
	gl.GetIntegerv(GL_TEXTURE_BINDING_2D, &prev);
	gl.BindTexture(GL_TEXTURE_2D, texture);
	gl.GetTexLevelParameteriv(GL_TEXTURE_2D, 0, GL_TEXTURE_WIDTH, &out->width);
	gl.GetTexLevelParameteriv(GL_TEXTURE_2D, 0, GL_TEXTURE_HEIGHT, &out->height);
	gl.BindTexture(GL_TEXTURE_2D, (GLuint)prev);
}

int obs_openvr_copy_gl_texture(GLuint texture, GLenum format, uint8_t *img)
{
	if (obs_openvr_gl_load() != 0)
		return -1;
	clear_errors();
	GLint prev = 0;
	gl.GetIntegerv(GL_TEXTURE_BINDING_2D, &prev);
	gl.BindTexture(GL_TEXTURE_2D, texture);
	gl.GetTexImage(GL_TEXTURE_2D, 0, format, GL_UNSIGNED_BYTE, img);
	GLenum status = gl.GetError();
	gl.BindTexture(GL_TEXTURE_2D, (GLuint)prev);
	return (int)status;
}

int obs_openvr_blit_gl_texture(GLuint src, GLint sx, GLint sy, GLsizei sw, GLsizei sh,
	GLuint dst, GLsizei dw, GLsizei dh)
{
	if (obs_openvr_gl_load() != 0)
		return -1;
	clear_errors();

	if (!gl.read_fbo)
		gl.GenFramebuffers(1, &gl.read_fbo);
	if (!gl.draw_fbo)
		gl.GenFramebuffers(1, &gl.draw_fbo);

	GLint prev_read = 0, prev_draw = 0;
	gl.GetIntegerv(GL_READ_FRAMEBUFFER_BINDING, &prev_read);
	gl.GetIntegerv(GL_DRAW_FRAMEBUFFER_BINDING, &prev_draw);

	gl.BindFramebuffer(GL_READ_FRAMEBUFFER, gl.read_fbo);
	gl.FramebufferTexture2D(GL_READ_FRAMEBUFFER, GL_COLOR_ATTACHMENT0, GL_TEXTURE_2D, src, 0);
	gl.BindFramebuffer(GL_DRAW_FRAMEBUFFER, gl.draw_fbo);
	gl.FramebufferTexture2D(GL_DRAW_FRAMEBUFFER, GL_COLOR_ATTACHMENT0, GL_TEXTURE_2D, dst, 0);

	int ret = 0;
	GLenum rs = gl.CheckFramebufferStatus(GL_READ_FRAMEBUFFER);
	GLenum ds = gl.CheckFramebufferStatus(GL_DRAW_FRAMEBUFFER);
	if (rs != GL_FRAMEBUFFER_COMPLETE) {
		ret = (int)rs;
	} else if (ds != GL_FRAMEBUFFER_COMPLETE) {
		ret = (int)ds;
	} else {
		GLenum filter = (sw == dw && sh == dh) ? GL_NEAREST : GL_LINEAR;
		gl.BlitFramebuffer(sx, sy, sx + sw, sy + sh, 0, 0, dw, dh, GL_COLOR_BUFFER_BIT, filter);
		ret = (int)gl.GetError();
	}

	/* Detach so we don't keep references to textures that may be freed. */
	gl.FramebufferTexture2D(GL_READ_FRAMEBUFFER, GL_COLOR_ATTACHMENT0, GL_TEXTURE_2D, 0, 0);
	gl.FramebufferTexture2D(GL_DRAW_FRAMEBUFFER, GL_COLOR_ATTACHMENT0, GL_TEXTURE_2D, 0, 0);
	gl.BindFramebuffer(GL_READ_FRAMEBUFFER, (GLuint)prev_read);
	gl.BindFramebuffer(GL_DRAW_FRAMEBUFFER, (GLuint)prev_draw);
	return ret;
}
