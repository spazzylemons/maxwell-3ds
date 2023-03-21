# Import this into Blender and go to File > Export > Maxwell

bl_info = {
    'name': 'Maxwell Exporter',
    'author': 'spazzylemons',
    'version': (1, 0, 0),
    'blender': (3, 4, 1),
    'location': 'File > Export',
    'description': 'funny cat',
    'category': 'Import-Export',
}

import bpy, bpy_extras, bmesh

class MaxwellExport(bpy.types.Operator, bpy_extras.io_utils.ExportHelper):
    """Export Maxwell."""
    bl_idname = 'maxwell.export'
    bl_label = 'Export Maxwell'

    filename_ext = '.c'

    def execute(self, context):
        try:
            # get the active mesh
            edit_mesh = bpy.context.edit_object.data
            mesh = bmesh.from_edit_mesh(edit_mesh)
            print(mesh)
            # get the points
            #with open(self.filepath, 'w') as file:
            uv_layer = mesh.loops.layers.uv.active
            loops = []
            seen = {}
            body_indices = []
            whiskers_indices = []
            for face in mesh.faces:
                if edit_mesh.materials[face.material_index].name == 'whiskers':
                    indices = whiskers_indices
                else:
                    indices = body_indices
                indices_in_tri = []
                for loop in face.loops:
                    vertex = (*loop.vert.co.xzy, *loop[uv_layer].uv)
                    if vertex not in seen:
                        seen[vertex] = len(seen)
                    index = seen[vertex]
                    indices_in_tri.append(index)
                    while len(loops) <= index:
                        loops.append(None)
                    loops[index] = vertex
                indices.append(indices_in_tri)
            with open(self.filepath, 'w') as file:
                print('const float maxwell_vertices[] = {', file=file)
                for loop in loops:
                    print('    ' + ''.join(str(x) + 'f,' for x in loop), file=file)
                print('};', file=file)
                print('const int maxwell_vertices_len = {};'.format(len(loops)), file=file)

                def do_indices(indices, name):
                    print('const short maxwell_{}_indices[] = {{'.format(name), file=file)
                    for face in indices:
                        print('    ' + ''.join(str(x) + ',' for x in face), file=file)
                    print('};', file=file)
                    print('const int maxwell_{}_indices_len = {};'.format(name, len(indices) * 3), file=file)

                do_indices(body_indices, 'body')
                do_indices(whiskers_indices, 'whiskers')
        except BaseException as e:
            self.report({'ERROR'}, repr(e))
            return {'CANCELLED'}
        else:
            return {'FINISHED'}

def maxwell_func(self, context):
    self.layout.operator(MaxwellExport.bl_idname)

def register():
    bpy.utils.register_class(MaxwellExport)
    bpy.types.TOPBAR_MT_file_export.append(maxwell_func)

def unregister():
    bpy.types.TOPBAR_MT_file_export.remove(maxwell_func)
    bpy.utils.unregister_class(MaxwellExport)
