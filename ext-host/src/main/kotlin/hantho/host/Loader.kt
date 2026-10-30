package hantho.host

import eu.kanade.tachiyomi.source.CatalogueSource
import eu.kanade.tachiyomi.source.Source
import eu.kanade.tachiyomi.source.SourceFactory
import java.io.File
import java.net.URLClassLoader
import java.util.concurrent.ConcurrentHashMap
import java.util.jar.JarFile

class LoadedExtension(val pkg: String, val jar: File, val sources: List<Source>)

object Loader {
    val sources = ConcurrentHashMap<Long, Source>()
    private val extensions = ConcurrentHashMap<String, LoadedExtension>()

    private val metaClass = Regex("""android:name="tachiyomi\.extension\.class"\s+android:value="([^"]+)"""")
    private val manifestPackage = Regex("""package="([^"]+)"""")

    fun load(jarPath: String): LoadedExtension {
        val jar = File(jarPath).absoluteFile
        val manifest = JarFile(jar).use { jf ->
            val entry = jf.getJarEntry("AndroidManifest.xml") ?: error("El jar no tiene AndroidManifest.xml")
            jf.getInputStream(entry).bufferedReader().readText()
        }
        val pkg = manifestPackage.find(manifest)?.groupValues?.get(1) ?: jar.nameWithoutExtension
        val classNames = metaClass.find(manifest)?.groupValues?.get(1)
            ?: error("Falta meta-data tachiyomi.extension.class")

        // Un classloader por extensión: cada jar trae su propia copia (ofuscada) de keiyoushi core
        val loader = URLClassLoader(arrayOf(jar.toURI().toURL()), Loader::class.java.classLoader)
        val created = classNames.split(';').map { it.trim() }.filter { it.isNotEmpty() }.flatMap { name ->
            val fqcn = if (name.startsWith(".")) pkg + name else name
            when (val obj = Class.forName(fqcn, false, loader).getDeclaredConstructor().newInstance()) {
                is Source -> listOf(obj)
                is SourceFactory -> obj.createSources()
                else -> error("$fqcn no es Source ni SourceFactory")
            }
        }
        created.forEach { sources[it.id] = it }
        return LoadedExtension(pkg, jar, created).also { extensions[pkg] = it }
    }

    fun source(id: Long): CatalogueSource =
        sources[id] as? CatalogueSource ?: error("Fuente $id no cargada")
}
