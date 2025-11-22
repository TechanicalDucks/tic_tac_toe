extends Node2D

func _ready():

	var rust_node = RustNode.new()
	add_child(rust_node)
	
	# Call Rust functions
	rust_node.hello_world()
	
   
